// `ato files-touched <run-id>` — which files did this dispatch modify?
//
// File attribution lives on the cloud `agent_traces.files_touched` JSONB
// column when the trace was uploaded. Local-side, the desktop captures
// mtime-snapshot diffs but doesn't always persist them to SQLite. For
// Phase 1, we look up the run by ID (matching either execution_logs.id
// or cloud_trace_id) and surface whatever file-attribution data is
// reachable from local tables. The fallback when nothing is local:
// suggest the agent re-fetch from the cloud `agent_traces` endpoint
// (which the desktop already does when populating Insights).

use crate::output::{emit_human, emit_json, Opts};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct FilesTouchedResult {
    pub run_id: String,
    pub cloud_trace_id: Option<String>,
    pub files: Vec<String>,
    pub source: String, // "local-cache" | "cloud-fetch-required" | "not-found"
    pub note: Option<String>,
}

pub fn run(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    // Try to find an execution_logs row to get a cloud_trace_id mapping.
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT id, cloud_trace_id FROM execution_logs
              WHERE id = ?1 OR cloud_trace_id = ?1
              LIMIT 1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let result = match row {
        None => FilesTouchedResult {
            run_id: id.to_string(),
            cloud_trace_id: None,
            files: vec![],
            source: "not-found".to_string(),
            note: Some(format!(
                "No execution_logs row matched id '{}'. Try `ato dispatches recent` to find available IDs.",
                id
            )),
        },
        Some((local_id, cloud_id)) => {
            // For Phase 1, local files_touched mirroring isn't shipped
            // yet — we always have to fetch from cloud. Be honest about
            // that in the response shape so agents can route to the
            // right endpoint.
            FilesTouchedResult {
                run_id: local_id,
                cloud_trace_id: cloud_id.clone(),
                files: vec![],
                source: if cloud_id.is_some() {
                    "cloud-fetch-required".to_string()
                } else {
                    "not-uploaded".to_string()
                },
                note: Some(if cloud_id.is_some() {
                    format!(
                        "File attribution is currently only persisted cloud-side. Fetch via the cloud API at /api/agent-traces/{} or open the trace in the desktop Insights → External tab. Local SQLite mirroring is on the Phase 1.x roadmap.",
                        cloud_id.as_deref().unwrap_or("")
                    )
                } else {
                    "This dispatch hasn't been uploaded to the cloud yet (no cloud_trace_id), so its file-attribution data isn't queryable from here. Sign in and the desktop will upload it on the next sync.".to_string()
                }),
            }
        }
    };

    if opts.human {
        emit_human(&format!(
            "Files touched for {}:\n  cloud_trace_id: {}\n  source: {}\n  files: {}\n  note: {}",
            result.run_id,
            result.cloud_trace_id.as_deref().unwrap_or("(none)"),
            result.source,
            if result.files.is_empty() {
                "(none locally available)".to_string()
            } else {
                result.files.join(", ")
            },
            result.note.as_deref().unwrap_or("")
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}
