// Output formatting: JSON is the default (agent-friendly, parseable).
// --human switches to a colored, terminal-friendly view.
//
// We prefer JSON-default because the primary consumer of this CLI is
// a coding agent shelling out from another process. The agent wants
// to pipe stdout through jq or parse it directly. Humans who run the
// CLI ad-hoc opt in with --human.

use anyhow::Result;
use serde::Serialize;

pub struct Opts {
    pub human: bool,
    pub quiet: bool,
}

/// Emit JSON to stdout for any serializable value. Pretty-print so
/// humans can also read it without --human; this costs ~nothing for
/// typical sizes (a few hundred rows).
pub fn emit_json<T: Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    println!("{}", s);
    Ok(())
}

/// Emit human-readable output. The exact format is up to each command
/// — this just writes through stdout. Commands choose between table-
/// style printing, colored summaries, etc. depending on what reads best.
pub fn emit_human(s: &str) {
    println!("{}", s);
}
