// ato — the local-first CLI for ATO.
//
// Talks to the same SQLite database (~/.ato/local.db) the desktop GUI
// reads/writes. Designed to be driven by humans AND coding agents:
// every meaningful operation outputs JSON to stdout by default
// (parseable), with a --human flag that switches to a readable
// terminal-friendly view.
//
// Status: Phase 1 of v2.3.0 (agent-driveable platform). Shipping the
// read-only Observation commands first; Operations + Authoring land
// in subsequent commits so each subcommand is reviewable in isolation.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod db;
mod output;
mod commands;

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

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine DB path: --db override → ~/.ato/local.db default.
    let db_path = cli
        .db
        .clone()
        .unwrap_or_else(db::default_db_path);

    // Open read-only by default; subcommands that need write access
    // reopen with write privileges.
    let conn = db::open_readonly(&db_path)
        .with_context(|| format!("Could not open ATO database at {}", db_path.display()))?;

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
            } => commands::dispatches::recent(&conn, limit, runtime, status, &opts),
        },
        Commands::Runs { sub } => match sub {
            RunsSub::Live => commands::runs::live(&conn, &opts),
            RunsSub::Get { id } => commands::runs::get(&conn, &id, &opts),
        },
        Commands::ConfigChanges { sub } => match sub {
            ConfigChangesSub::List { agent, since } => {
                commands::config_changes::list(&conn, &agent, &since, &opts)
            }
        },
        Commands::FilesTouched { id } => commands::files_touched::run(&conn, &id, &opts),
        Commands::Replays { sub } => match sub {
            ReplaysSub::ForTrace { trace_id } => {
                commands::replays::for_trace(&conn, &trace_id, &opts)
            }
        },
    }
}
