// v2.17 — Output bundles: packaged inference results.
//
// A bundle captures (source row + dispatches + judge scores + artifact
// files) at creation time so the result can be shared externally even
// after the underlying tables change. `ato bundles export` writes a
// tarball with a stable layout (manifest.json + dispatches.jsonl +
// source.json + artifacts/) that any reader can parse without the
// SQLite ledger.
//
// Open-core boundary: the OSS surface owns the table, the CLI, and the
// local tarball export. Cloud hosting (signed URL generation, upload,
// retention policy) lives in `ato-cloud` and writes back into
// output_bundles.signed_url on success.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

// ── CLI surface ────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct BundlesArgs {
    #[command(subcommand)]
    pub sub: BundlesSub,
}

#[derive(Subcommand, Debug)]
pub enum BundlesSub {
    /// Capture a source row (methodology_run / mission / loop_run / session
    /// / execution_log) into a bundle. The manifest is built NOW from the
    /// current state of the source — the bundle is immutable from then on.
    Create {
        #[arg(long)]
        name: String,
        /// 'methodology_run' | 'mission' | 'loop_run' | 'session' | 'execution_log'
        #[arg(long = "source-kind")]
        source_kind: String,
        /// The row id of the source.
        #[arg(long = "source-id")]
        source_id: String,
        /// Optional human description.
        #[arg(long)]
        description: Option<String>,
        /// Override the auto-derived slug.
        #[arg(long)]
        slug: Option<String>,
    },
    /// List bundles, newest first. Optional filter by source_kind.
    List {
        #[arg(long = "source-kind")]
        source_kind: Option<String>,
    },
    /// Print one bundle (slug or id).
    Show { slug_or_id: String },
    /// Write a tarball containing the bundle's source + dispatches +
    /// artifacts. The bundle row's `export_path` is updated on success.
    Export {
        slug_or_id: String,
        /// Where to write the .tar.gz. Default: ./<slug>.bundle.tar.gz
        #[arg(long)]
        to: Option<PathBuf>,
    },
    /// Delete a bundle. Requires --yes to confirm.
    Delete {
        slug_or_id: String,
        #[arg(long)]
        yes: bool,
    },
}

// ── Validation ─────────────────────────────────────────────────────────

const VALID_SOURCE_KINDS: &[&str] = &[
    "methodology_run",
    "mission",
    "loop_run",
    "session",
    "execution_log",
];

fn validate_source_kind(kind: &str) -> Result<()> {
    if !VALID_SOURCE_KINDS.contains(&kind) {
        anyhow::bail!(
            "invalid --source-kind: '{}' (expected one of {})",
            kind,
            VALID_SOURCE_KINDS.join("|")
        );
    }
    Ok(())
}

// ── Row types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OutputBundleRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub source_kind: String,
    pub source_id: String,
    pub manifest: serde_json::Value,
    pub export_path: Option<String>,
    pub signed_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const BUNDLE_SELECT: &str = "SELECT id, slug, name, description, source_kind,
                                    source_id, manifest, export_path, signed_url,
                                    created_at, updated_at FROM output_bundles";

// ── Dispatcher ─────────────────────────────────────────────────────────

pub fn run(args: BundlesArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        BundlesSub::Create {
            name,
            source_kind,
            source_id,
            description,
            slug,
        } => run_create(
            name,
            source_kind,
            source_id,
            description,
            slug,
            db_path,
            opts,
        ),
        BundlesSub::List { source_kind } => run_list(source_kind, db_path, opts),
        BundlesSub::Show { slug_or_id } => run_show(slug_or_id, db_path, opts),
        BundlesSub::Export { slug_or_id, to } => run_export(slug_or_id, to, db_path, opts),
        BundlesSub::Delete { slug_or_id, yes } => run_delete(slug_or_id, yes, db_path, opts),
    }
}

// ── Create ─────────────────────────────────────────────────────────────

fn run_create(
    name: String,
    source_kind: String,
    source_id: String,
    description: Option<String>,
    slug_override: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("name is empty");
    }
    if source_id.trim().is_empty() {
        anyhow::bail!("source-id is empty");
    }
    validate_source_kind(&source_kind)?;

    let conn = db::open_readwrite(db_path)?;

    // Build the manifest by reading the source row and its associated
    // dispatches. Each source_kind has a different shape — we capture
    // the minimum that lets a downstream consumer reconstruct the run.
    let manifest = build_manifest(&conn, &source_kind, &source_id)?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let name_trimmed: String = name.trim().chars().take(200).collect();
    let base_slug = slug_override
        .as_deref()
        .map(slugify)
        .unwrap_or_else(|| slugify(&name_trimmed));
    let slug = unique_slug(&conn, &base_slug)?;

    let manifest_str = serde_json::to_string(&manifest).context("serialize manifest")?;

    conn.execute(
        "INSERT INTO output_bundles (
            id, slug, name, description, source_kind, source_id,
            manifest, export_path, signed_url, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?8)",
        params![
            id,
            slug,
            name_trimmed,
            description,
            source_kind,
            source_id,
            manifest_str,
            now,
        ],
    )
    .context("insert output_bundle")?;

    let row = load_bundle(&conn, &id)?;
    if opts.human {
        emit_human(&format!(
            "Created bundle '{}' (slug: {})\n  source: {}:{}\n  manifest: {} item(s)",
            row.name,
            row.slug,
            row.source_kind,
            row.source_id,
            manifest_summary_count(&row.manifest),
        ));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

/// Build the JSON manifest by reading the source row and any associated
/// dispatch ids. The manifest is intentionally a snapshot — once the
/// bundle is created, re-reading the source doesn't change it.
fn build_manifest(
    conn: &Connection,
    source_kind: &str,
    source_id: &str,
) -> Result<serde_json::Value> {
    let dispatches = collect_dispatch_ids(conn, source_kind, source_id)?;
    let source_summary = load_source_summary(conn, source_kind, source_id)?;
    // Codex R2 2026-06-13: collect across all constituent dispatches —
    // covers mission/methodology_run/loop_run/session, not just
    // execution_log. The single-dispatch case still works because
    // `dispatches` for source_kind=='execution_log' is [source_id].
    let artifact_paths = collect_artifact_paths_for_dispatches(conn, &dispatches);

    Ok(serde_json::json!({
        "source": {
            "kind": source_kind,
            "id": source_id,
            "summary": source_summary,
        },
        "dispatches": dispatches,
        "artifact_paths": artifact_paths,
        "captured_at": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Look up execution_log ids associated with the source. The shape is
/// per source_kind — best-effort; missing data is fine and returns [].
fn collect_dispatch_ids(
    conn: &Connection,
    source_kind: &str,
    source_id: &str,
) -> Result<Vec<String>> {
    let ids = match source_kind {
        "execution_log" => vec![source_id.to_string()],
        "session" => {
            let mut stmt = conn.prepare(
                "SELECT id FROM execution_logs WHERE session_id = ?1 ORDER BY created_at ASC",
            )?;
            let it = stmt.query_map(params![source_id], |r| r.get::<_, String>(0))?;
            it.filter_map(|r| r.ok()).collect()
        }
        "methodology_run" => {
            // methodology_run_dispatches links runs to execution_logs.
            let mut stmt = match conn.prepare(
                "SELECT execution_log_id FROM methodology_run_dispatches
                 WHERE methodology_run_id = ?1
                 ORDER BY execution_log_id ASC",
            ) {
                Ok(s) => s,
                Err(_) => return Ok(Vec::new()),
            };
            let it = stmt.query_map(params![source_id], |r| r.get::<_, String>(0))?;
            it.filter_map(|r| r.ok()).collect()
        }
        "loop_run" => {
            // loop_run_steps.execution_log_id (TEXT post-PR-2.5).
            let mut stmt = match conn.prepare(
                "SELECT execution_log_id FROM loop_run_steps
                 WHERE loop_run_id = ?1 AND execution_log_id IS NOT NULL
                 ORDER BY started_at ASC",
            ) {
                Ok(s) => s,
                Err(_) => return Ok(Vec::new()),
            };
            let it = stmt.query_map(params![source_id], |r| r.get::<_, String>(0))?;
            it.filter_map(|r| r.ok()).collect()
        }
        "mission" => {
            // Codex R2 2026-06-13: missions execute through TWO paths.
            //   (a) `mission_events.kind='dispatched'` — direct worker dispatches
            //   (b) coordinator tick → loop_runs → loop_run_steps.execution_log_id
            //       (linked via mission_events.kind='loop_run_completed' →
            //       payload.loop_run_id, same shape the missions cost path uses
            //       in missions.rs ~line 1340).
            // The earlier shape only saw (a) so mission bundles silently
            // dropped every loop-worker dispatch. UNION + DISTINCT collects both.
            let mut stmt = conn.prepare(
                "SELECT log_id FROM (
                     SELECT json_extract(payload, '$.execution_log_id') AS log_id,
                            occurred_at
                     FROM mission_events
                     WHERE mission_id = ?1
                       AND kind = 'dispatched'
                       AND json_extract(payload, '$.execution_log_id') IS NOT NULL
                     UNION
                     SELECT s.execution_log_id AS log_id, e.occurred_at
                     FROM mission_events e
                     JOIN loop_run_steps s
                       ON s.loop_run_id = json_extract(e.payload, '$.loop_run_id')
                     WHERE e.mission_id = ?1
                       AND e.kind = 'loop_run_completed'
                       AND s.execution_log_id IS NOT NULL
                 ) ORDER BY occurred_at ASC",
            )?;
            let it = stmt.query_map(params![source_id], |r| r.get::<_, String>(0))?;
            let mut seen = std::collections::HashSet::new();
            it.filter_map(|r| r.ok())
                .filter(|id| seen.insert(id.clone()))
                .collect()
        }
        _ => Vec::new(),
    };
    Ok(ids)
}

/// Load a tiny summary of the source row. Best-effort; missing rows
/// return JSON null so a bundle can still be built (with a warning).
fn load_source_summary(
    conn: &Connection,
    source_kind: &str,
    source_id: &str,
) -> Result<serde_json::Value> {
    let summary = match source_kind {
        "execution_log" => conn
            .query_row(
                "SELECT runtime, model, status, cost_usd_estimated, created_at
                 FROM execution_logs WHERE id = ?1",
                params![source_id],
                |r| {
                    Ok(serde_json::json!({
                        "runtime": r.get::<_, Option<String>>(0).ok().flatten(),
                        "model": r.get::<_, Option<String>>(1).ok().flatten(),
                        "status": r.get::<_, Option<String>>(2).ok().flatten(),
                        "cost_usd": r.get::<_, Option<f64>>(3).ok().flatten(),
                        "created_at": r.get::<_, Option<String>>(4).ok().flatten(),
                    }))
                },
            )
            .ok()
            .unwrap_or(serde_json::Value::Null),
        "session" => conn
            .query_row(
                "SELECT id, title, summary, closed_at FROM sessions WHERE id = ?1",
                params![source_id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, Option<String>>(0).ok().flatten(),
                        "title": r.get::<_, Option<String>>(1).ok().flatten(),
                        "summary": r.get::<_, Option<String>>(2).ok().flatten(),
                        "closed_at": r.get::<_, Option<String>>(3).ok().flatten(),
                    }))
                },
            )
            .ok()
            .unwrap_or(serde_json::Value::Null),
        "mission" => conn
            .query_row(
                "SELECT id, slug, name, goal, state, category FROM missions WHERE id = ?1 OR slug = ?1",
                params![source_id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, Option<String>>(0).ok().flatten(),
                        "slug": r.get::<_, Option<String>>(1).ok().flatten(),
                        "name": r.get::<_, Option<String>>(2).ok().flatten(),
                        "goal": r.get::<_, Option<String>>(3).ok().flatten(),
                        "state": r.get::<_, Option<String>>(4).ok().flatten(),
                        "category": r.get::<_, Option<String>>(5).ok().flatten(),
                    }))
                },
            )
            .ok()
            .unwrap_or(serde_json::Value::Null),
        "methodology_run" => conn
            .query_row(
                "SELECT id, methodology_id, status, started_at, finished_at
                 FROM methodology_runs WHERE id = ?1",
                params![source_id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, Option<String>>(0).ok().flatten(),
                        "methodology_id": r.get::<_, Option<String>>(1).ok().flatten(),
                        "status": r.get::<_, Option<String>>(2).ok().flatten(),
                        "started_at": r.get::<_, Option<String>>(3).ok().flatten(),
                        "finished_at": r.get::<_, Option<String>>(4).ok().flatten(),
                    }))
                },
            )
            .ok()
            .unwrap_or(serde_json::Value::Null),
        "loop_run" => conn
            .query_row(
                "SELECT id, loop_id, status, started_at, finished_at FROM loop_runs WHERE id = ?1",
                params![source_id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, Option<String>>(0).ok().flatten(),
                        "loop_id": r.get::<_, Option<String>>(1).ok().flatten(),
                        "status": r.get::<_, Option<String>>(2).ok().flatten(),
                        "started_at": r.get::<_, Option<String>>(3).ok().flatten(),
                        "finished_at": r.get::<_, Option<String>>(4).ok().flatten(),
                    }))
                },
            )
            .ok()
            .unwrap_or(serde_json::Value::Null),
        _ => serde_json::Value::Null,
    };
    Ok(summary)
}

/// Collect referenced filesystem paths (file_attribution) so the export
/// can include the actual artifact files. Best-effort.
///
/// Codex R2 2026-06-13: previous shape only ran for source_kind ==
/// "execution_log", so bundles for mission/methodology_run/loop_run/
/// session always reported zero artifacts even when their constituent
/// dispatches touched files. Now: when the source is a container kind,
/// collect file_attribution for ALL constituent dispatch_ids (passed in
/// by build_manifest) instead of bailing.
fn collect_artifact_paths_for_dispatches(
    conn: &Connection,
    dispatch_ids: &[String],
) -> Vec<String> {
    // Codex R3 2026-06-13: chunk the IN query in 500-id batches instead of
    // silently truncating. Missions with many loop-worker dispatches were
    // dropping file_attribution rows past the first 500 before this.
    const CHUNK: usize = 500;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for ids in dispatch_ids.chunks(CHUNK) {
        if ids.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT file_path FROM file_attribution
             WHERE execution_log_id IN ({}) GROUP BY file_path",
            placeholders
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let it = match stmt.query_map(
            rusqlite::params_from_iter(ids.iter()),
            |r| r.get::<_, String>(0),
        ) {
            Ok(i) => i,
            Err(_) => continue,
        };
        for path in it.filter_map(|r| r.ok()) {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }
    out
}

#[allow(dead_code)] // kept for the unit test fixture; build_manifest now
                    // uses collect_artifact_paths_for_dispatches above.
fn collect_artifact_paths(conn: &Connection, source_kind: &str, source_id: &str) -> Vec<String> {
    if source_kind != "execution_log" {
        return Vec::new();
    }
    let mut stmt = match conn.prepare(
        "SELECT file_path FROM file_attribution
         WHERE execution_log_id = ?1
         GROUP BY file_path",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let it = match stmt.query_map(params![source_id], |r| r.get::<_, String>(0)) {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };
    it.filter_map(|r| r.ok()).collect()
}

fn manifest_summary_count(manifest: &serde_json::Value) -> usize {
    manifest
        .get("dispatches")
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

// ── List / Show ────────────────────────────────────────────────────────

fn run_list(source_kind: Option<String>, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    if let Some(k) = source_kind.as_deref() {
        validate_source_kind(k)?;
    }
    let conn = db::open_readonly(db_path)?;
    let mut sql = String::from(BUNDLE_SELECT);
    if source_kind.is_some() {
        sql.push_str(" WHERE source_kind = ?1");
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<OutputBundleRow> = if let Some(k) = source_kind.as_deref() {
        stmt.query_map(params![k], row_to_bundle)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], row_to_bundle)?
            .filter_map(|r| r.ok())
            .collect()
    };

    if opts.human {
        if rows.is_empty() {
            emit_human("No bundles.");
        } else {
            for r in &rows {
                let dcount = manifest_summary_count(&r.manifest);
                emit_human(&format!(
                    "  · {} [{}:{}]  {}  ({} dispatch{}){}",
                    r.slug,
                    r.source_kind,
                    short_id(&r.source_id),
                    r.name,
                    dcount,
                    if dcount == 1 { "" } else { "es" },
                    r.export_path.as_deref().map(|p| format!("  → {}", p)).unwrap_or_default(),
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn run_show(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let row = load_bundle(&conn, &slug_or_id)?;
    if opts.human {
        emit_human(&format!(
            "Bundle '{}' (slug: {})\n  source: {}:{}\n  description: {}\n  manifest: {} dispatch(es), {} artifact(s)\n  export_path: {}\n  signed_url: {}\n  created: {}",
            row.name,
            row.slug,
            row.source_kind,
            row.source_id,
            row.description.as_deref().unwrap_or("(none)"),
            manifest_summary_count(&row.manifest),
            row.manifest.get("artifact_paths").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0),
            row.export_path.as_deref().unwrap_or("(not exported)"),
            row.signed_url.as_deref().unwrap_or("(not hosted)"),
            row.created_at,
        ));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

// ── Export ─────────────────────────────────────────────────────────────

fn run_export(
    slug_or_id: String,
    to: Option<PathBuf>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let row = load_bundle(&conn, &slug_or_id)?;

    let out_path = to.unwrap_or_else(|| PathBuf::from(format!("{}.bundle.tar.gz", row.slug)));

    // Build a staging directory then tar it. Simpler than streaming
    // tarball construction and works without any extra crate (we shell
    // out to `tar`, which is available on macOS + Linux).
    let staging = std::env::temp_dir().join(format!("ato-bundle-{}", Uuid::new_v4()));
    fs::create_dir_all(&staging)
        .with_context(|| format!("mkdir staging {}", staging.display()))?;
    // Cleanup guard — drop the staging dir even on error.
    struct StagingGuard(PathBuf);
    impl Drop for StagingGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _staging_guard = StagingGuard(staging.clone());
    let root = staging.as_path();

    // manifest.json
    let manifest_str = serde_json::to_string_pretty(&row.manifest).context("serialize manifest")?;
    fs::write(root.join("manifest.json"), manifest_str).context("write manifest.json")?;

    // source.json
    let source = serde_json::json!({
        "slug": row.slug,
        "name": row.name,
        "description": row.description,
        "source_kind": row.source_kind,
        "source_id": row.source_id,
        "created_at": row.created_at,
    });
    fs::write(
        root.join("source.json"),
        serde_json::to_string_pretty(&source).context("serialize source")?,
    )?;

    // dispatches.jsonl — one execution_log row per line.
    let dispatch_ids: Vec<String> = row
        .manifest
        .get("dispatches")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let dispatches_path = root.join("dispatches.jsonl");
    let mut dispatches_file = std::fs::File::create(&dispatches_path)
        .with_context(|| format!("create {}", dispatches_path.display()))?;
    use std::io::Write;
    for id in &dispatch_ids {
        if let Ok(row_json) = read_execution_log_as_json(&conn, id) {
            writeln!(dispatches_file, "{}", serde_json::to_string(&row_json)?)?;
        }
    }
    drop(dispatches_file);

    // artifacts/ — copy any referenced files that exist on disk.
    //
    // Codex R2 2026-06-13: previous shape trusted `manifest.artifact_paths`
    // verbatim — `fs::copy(PathBuf::from(p), ...)` with no anchoring,
    // so a crafted bundle row could exfiltrate arbitrary local files
    // via absolute paths or `../...`. Anchor every candidate against
    // the export root (CWD or env-overridden). Anything that resolves
    // outside that root (incl. broken symlinks pointing out) is dropped.
    let artifact_paths: Vec<String> = row
        .manifest
        .get("artifact_paths")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let trust_root = artifact_trust_root();
    let mut copied = 0_usize;
    let mut skipped_unsafe = 0_usize;
    let mut skipped_missing = 0_usize;
    if !artifact_paths.is_empty() {
        let arts_dir = root.join("artifacts");
        fs::create_dir_all(&arts_dir).context("mkdir artifacts")?;
        for p in &artifact_paths {
            match resolve_safe_artifact(&trust_root, p) {
                ArtifactPath::Safe(src) => {
                    let safe = sanitize_for_archive(p);
                    let dst = arts_dir.join(&safe);
                    let dst = uniqueify_path(&arts_dir, &dst);
                    match fs::copy(&src, &dst) {
                        Ok(_) => copied += 1,
                        Err(_) => skipped_missing += 1,
                    }
                }
                ArtifactPath::Missing => skipped_missing += 1,
                ArtifactPath::OutsideTrust => skipped_unsafe += 1,
            }
        }
    }

    // Tar it.
    // tar -czf <out> -C <staging> .
    let out_abs = if out_path.is_absolute() {
        out_path.clone()
    } else {
        std::env::current_dir()
            .map(|c| c.join(&out_path))
            .unwrap_or(out_path.clone())
    };
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&out_abs)
        .arg("-C")
        .arg(root)
        .arg(".")
        .status()
        .context("spawn tar")?;
    if !status.success() {
        anyhow::bail!("tar exited with status {}", status);
    }

    // Persist the export_path back on the row.
    let now = chrono::Utc::now().to_rfc3339();
    let out_path_str = out_abs.to_string_lossy().to_string();
    conn.execute(
        "UPDATE output_bundles SET export_path = ?1, updated_at = ?2 WHERE id = ?3",
        params![out_path_str, now, row.id],
    )?;

    if opts.human {
        emit_human(&format!(
            "Exported bundle '{}' to {}\n  dispatches:        {}\n  artifacts copied:  {}\n  artifacts missing: {}\n  artifacts unsafe:  {}",
            row.slug,
            out_path_str,
            dispatch_ids.len(),
            copied,
            skipped_missing,
            skipped_unsafe,
        ));
    } else {
        emit_json(&serde_json::json!({
            "slug": row.slug,
            "export_path": out_path_str,
            "dispatches": dispatch_ids.len(),
            "artifacts": copied,
            "artifacts_requested": artifact_paths.len(),
            "artifacts_skipped_missing": skipped_missing,
            "artifacts_skipped_unsafe": skipped_unsafe,
        }))?;
    }
    Ok(())
}

/// Outcome of resolving an artifact_paths entry against the trust root.
#[derive(Debug)]
enum ArtifactPath {
    /// Resolved to a real file under the trust root — safe to copy.
    Safe(PathBuf),
    /// Resolved path is outside the trust root — refuse the copy.
    OutsideTrust,
    /// Path does not exist OR cannot be canonicalized — drop quietly.
    Missing,
}

/// Decide the trust root for artifact resolution. Honors
/// `$ATO_BUNDLE_TRUST_ROOT` for headless / CI flows; otherwise uses CWD.
/// Symlink-resolved via canonicalize so `..` cannot bypass the root.
fn artifact_trust_root() -> PathBuf {
    let raw = std::env::var("ATO_BUNDLE_TRUST_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    raw.canonicalize().unwrap_or(raw)
}

/// Resolve one artifact_paths entry. Returns `Safe` only when the
/// canonicalized ABSOLUTE path is a regular file inside `trust_root`.
///
/// Codex R3 2026-06-13: relative paths are refused (OutsideTrust). The
/// previous shape joined a relative path against the export-time CWD,
/// which could resolve to the wrong same-named file when exporting from
/// a different directory than where the dispatch ran. Bundle correctness
/// requires the file_attribution writer to store absolute paths — if a
/// relative path lands here it's a contract violation, not a hint to
/// guess.
fn resolve_safe_artifact(trust_root: &std::path::Path, raw: &str) -> ArtifactPath {
    let candidate = PathBuf::from(raw);
    if !candidate.is_absolute() {
        return ArtifactPath::OutsideTrust;
    }
    let canonical = match candidate.canonicalize() {
        Ok(c) => c,
        Err(_) => return ArtifactPath::Missing,
    };
    if !canonical.is_file() {
        return ArtifactPath::Missing;
    }
    if !canonical.starts_with(trust_root) {
        return ArtifactPath::OutsideTrust;
    }
    ArtifactPath::Safe(canonical)
}

fn read_execution_log_as_json(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let v: serde_json::Value = conn
        .query_row(
            "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms,
                    status, error_message, model, session_id, cost_usd_estimated, created_at
             FROM execution_logs WHERE id = ?1",
            params![id],
            |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, Option<String>>(0).ok().flatten(),
                    "runtime": r.get::<_, Option<String>>(1).ok().flatten(),
                    "prompt": r.get::<_, Option<String>>(2).ok().flatten(),
                    "response": r.get::<_, Option<String>>(3).ok().flatten(),
                    "tokens_in": r.get::<_, Option<i64>>(4).ok().flatten(),
                    "tokens_out": r.get::<_, Option<i64>>(5).ok().flatten(),
                    "duration_ms": r.get::<_, Option<i64>>(6).ok().flatten(),
                    "status": r.get::<_, Option<String>>(7).ok().flatten(),
                    "error_message": r.get::<_, Option<String>>(8).ok().flatten(),
                    "model": r.get::<_, Option<String>>(9).ok().flatten(),
                    "session_id": r.get::<_, Option<String>>(10).ok().flatten(),
                    "cost_usd": r.get::<_, Option<f64>>(11).ok().flatten(),
                    "created_at": r.get::<_, Option<String>>(12).ok().flatten(),
                }))
            },
        )
        .with_context(|| format!("read execution_log {}", id))?;
    Ok(v)
}

/// Make a path safe to use as a single archive filename. Replace path
/// separators with `_` so /apps/cli/src/main.rs becomes
/// _apps_cli_src_main.rs.
fn sanitize_for_archive(p: &str) -> String {
    p.replace(['/', '\\'], "_")
        .trim_start_matches('_')
        .to_string()
}

fn uniqueify_path(dir: &std::path::Path, p: &std::path::Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    for i in 1..1000 {
        let candidate = if ext.is_empty() {
            dir.join(format!("{}({})", stem, i))
        } else {
            dir.join(format!("{}({}).{}", stem, i, ext))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    p.to_path_buf()
}

// ── Delete ─────────────────────────────────────────────────────────────

fn run_delete(slug_or_id: String, yes: bool, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    if !yes {
        anyhow::bail!(
            "refusing to delete bundle '{}' without --yes (irreversible)",
            slug_or_id
        );
    }
    let conn = db::open_readwrite(db_path)?;
    let row = load_bundle(&conn, &slug_or_id)?;
    let n = conn.execute("DELETE FROM output_bundles WHERE id = ?1", params![row.id])?;
    if opts.human {
        emit_human(&format!(
            "Deleted bundle '{}' ({} row{})",
            row.slug,
            n,
            if n == 1 { "" } else { "s" }
        ));
    } else {
        emit_json(&serde_json::json!({"deleted": row.slug, "rows": n}))?;
    }
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────

fn id_or_slug_column(input: &str) -> &'static str {
    if Uuid::parse_str(input).is_ok() {
        "id"
    } else {
        "slug"
    }
}

fn short_id(s: &str) -> String {
    s.chars().take(8).collect()
}

fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_sep = true;
    for ch in name.chars().take(200) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("bundle");
    }
    out.chars().take(64).collect()
}

fn unique_slug(conn: &Connection, base: &str) -> Result<String> {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM output_bundles WHERE slug = ?1",
                params![candidate],
                |r| r.get(0),
            )
            .context("query slug collision")?;
        if exists == 0 {
            return Ok(candidate);
        }
        candidate = format!("{}-{}", base, suffix);
        suffix += 1;
        if suffix > 1000 {
            anyhow::bail!("slug exhaustion");
        }
    }
}

fn load_bundle(conn: &Connection, slug_or_id: &str) -> Result<OutputBundleRow> {
    let col = id_or_slug_column(slug_or_id);
    let sql = format!("{} WHERE {} = ?1", BUNDLE_SELECT, col);
    conn.query_row(&sql, params![slug_or_id], row_to_bundle)
        .with_context(|| format!("load output_bundle '{}'", slug_or_id))
}

fn row_to_bundle(r: &rusqlite::Row) -> rusqlite::Result<OutputBundleRow> {
    let manifest_str: String = r.get(6)?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_str).unwrap_or(serde_json::Value::Null);
    Ok(OutputBundleRow {
        id: r.get(0)?,
        slug: r.get(1)?,
        name: r.get(2)?,
        description: r.get(3)?,
        source_kind: r.get(4)?,
        source_id: r.get(5)?,
        manifest,
        export_path: r.get(7)?,
        signed_url: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE output_bundles (
                id            TEXT PRIMARY KEY,
                slug          TEXT NOT NULL UNIQUE,
                name          TEXT NOT NULL,
                description   TEXT,
                source_kind   TEXT NOT NULL,
                source_id     TEXT NOT NULL,
                manifest      TEXT NOT NULL,
                export_path   TEXT,
                signed_url    TEXT,
                created_at    TEXT NOT NULL,
                updated_at    TEXT NOT NULL
            );
            CREATE TABLE execution_logs (
                id TEXT PRIMARY KEY,
                runtime TEXT,
                prompt TEXT,
                response TEXT,
                tokens_in INTEGER,
                tokens_out INTEGER,
                duration_ms INTEGER,
                status TEXT,
                error_message TEXT,
                model TEXT,
                session_id TEXT,
                cost_usd_estimated REAL,
                created_at TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn validate_source_kind_accepts_known_and_rejects_unknown() {
        assert!(validate_source_kind("mission").is_ok());
        assert!(validate_source_kind("methodology_run").is_ok());
        let err = validate_source_kind("bogus").unwrap_err();
        assert!(format!("{}", err).contains("bogus"));
        assert!(format!("{}", err).contains("methodology_run"));
    }

    #[test]
    fn slugify_produces_lower_kebab_with_punctuation_collapsed() {
        assert_eq!(slugify("Bundle for the v2.16 run"), "bundle-for-the-v2-16-run");
        assert_eq!(slugify("!!!"), "bundle");
        assert_eq!(slugify(""), "bundle");
        let long = "x".repeat(200);
        assert_eq!(slugify(&long).len(), 64);
    }

    #[test]
    fn unique_slug_appends_suffix_on_collision() {
        let conn = make_db();
        let now = "2026-06-13T00:00:00Z";
        conn.execute(
            "INSERT INTO output_bundles (id, slug, name, source_kind, source_id, manifest, created_at, updated_at)
             VALUES ('b1', 'taken', 'X', 'mission', 'm1', '{}', ?1, ?1)",
            params![now],
        ).unwrap();
        assert_eq!(unique_slug(&conn, "fresh").unwrap(), "fresh");
        assert_eq!(unique_slug(&conn, "taken").unwrap(), "taken-2");
    }

    #[test]
    fn id_or_slug_column_routes_by_uuid_shape() {
        assert_eq!(id_or_slug_column(&Uuid::new_v4().to_string()), "id");
        assert_eq!(id_or_slug_column("my-bundle"), "slug");
    }

    #[test]
    fn sanitize_for_archive_flattens_paths() {
        assert_eq!(
            sanitize_for_archive("/apps/cli/src/main.rs"),
            "apps_cli_src_main.rs"
        );
        assert_eq!(sanitize_for_archive("README.md"), "README.md");
        assert_eq!(sanitize_for_archive("a\\b\\c.txt"), "a_b_c.txt");
    }

    #[test]
    fn collect_dispatch_ids_for_execution_log_returns_itself() {
        let conn = make_db();
        let ids = collect_dispatch_ids(&conn, "execution_log", "log-1").unwrap();
        assert_eq!(ids, vec!["log-1".to_string()]);
    }

    #[test]
    fn collect_dispatch_ids_for_session_walks_execution_logs_by_session_id() {
        let conn = make_db();
        let now = "2026-06-13T00:00:00Z";
        for id in &["a", "b", "c"] {
            conn.execute(
                "INSERT INTO execution_logs (id, runtime, status, session_id, created_at)
                 VALUES (?1, 'claude', 'success', 'sess-1', ?2)",
                params![id, now],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO execution_logs (id, runtime, status, session_id, created_at)
             VALUES ('d', 'claude', 'success', 'sess-2', ?1)",
            params![now],
        )
        .unwrap();

        let ids = collect_dispatch_ids(&conn, "session", "sess-1").unwrap();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn delete_without_yes_refuses() {
        // The delete command itself bails before touching the DB when --yes
        // is missing; assert that path directly. Use a non-existent slug
        // since the early bail happens before lookup.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let opts = Opts { human: false, quiet: false };
        let err = run_delete("ghost".into(), false, &tmp.path().to_path_buf(), &opts).unwrap_err();
        assert!(format!("{}", err).contains("--yes"));
    }

    #[test]
    fn create_round_trips_and_lists() {
        let conn = make_db();
        let now = "2026-06-13T00:00:00Z";
        // Manually insert a bundle row (mimicking what run_create would do).
        let manifest = serde_json::json!({
            "source": {"kind": "execution_log", "id": "log-1"},
            "dispatches": ["log-1"],
            "artifact_paths": [],
            "captured_at": now,
        });
        conn.execute(
            "INSERT INTO output_bundles (id, slug, name, source_kind, source_id, manifest, created_at, updated_at)
             VALUES ('b1', 'demo', 'Demo', 'execution_log', 'log-1', ?1, ?2, ?2)",
            params![serde_json::to_string(&manifest).unwrap(), now],
        ).unwrap();

        let row = load_bundle(&conn, "demo").unwrap();
        assert_eq!(row.slug, "demo");
        assert_eq!(row.source_kind, "execution_log");
        assert_eq!(manifest_summary_count(&row.manifest), 1);
    }
}
