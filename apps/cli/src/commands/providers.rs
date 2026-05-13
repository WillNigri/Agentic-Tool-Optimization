// v2.4.2 — `ato providers test` smoke-tests every api-provider
// against a one-token roundtrip to catch the failure mode that bit
// us when google/Gemini landed: provider in the registry but
// nobody ever actually called it end-to-end, so by the time a user
// tried it the default model had been deprecated upstream.
//
// What this command does for each provider:
//   1. Try to resolve an API key (env var → llm_api_keys table).
//      If no key, skip with status="no_key".
//   2. Dispatch a tiny prompt that should produce a 1-3 token reply.
//   3. Report success / error with the actual HTTP status / model
//      version used / round-trip duration.
//
// Designed to drop into QA.md §1 as a pre-release check so a new
// provider can't ship without being demonstrably reachable. Also
// useful for users diagnosing "my key isn't working" without
// burning their main prompt path.

use anyhow::Result;
use serde::Serialize;

use crate::api_dispatch::{dispatch_with_history, resolve_api_key, ApiProvider};
use crate::db;
use crate::output::{emit_human, emit_json, Opts};
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct ProviderTestRow {
    pub slug: &'static str,
    pub label: String,
    pub default_model: &'static str,
    pub flavor: &'static str,
    /// One of: `ok` / `no_key` / `http_error` / `dispatch_error`.
    pub status: String,
    /// Short description of what happened — present on every status,
    /// not just errors, so the human output reads consistently.
    pub detail: String,
    pub duration_ms: Option<i64>,
}

const SMOKE_PROMPT: &str =
    "Respond with exactly the single word 'ok' and nothing else — no punctuation, no quotes.";

pub fn run(db_path: &PathBuf, only_slug: Option<&str>, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let providers: Vec<&ApiProvider> = ato_api_providers::registry()
        .iter()
        .filter(|p| match only_slug {
            Some(s) => p.slug == s.to_ascii_lowercase(),
            None => true,
        })
        .collect();

    if providers.is_empty() {
        anyhow::bail!(
            "No matching provider. Known slugs: {}",
            ato_api_providers::registry()
                .iter()
                .map(|p| p.slug)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let mut rows: Vec<ProviderTestRow> = Vec::with_capacity(providers.len());
    let mut any_fail = false;
    for provider in providers {
        let label = ato_api_providers::label_for(provider.slug)
            .unwrap_or(provider.slug)
            .to_string();

        // Key check first — skipping cleanly when no key is configured
        // is the most common case and shouldn't read as failure.
        if resolve_api_key(provider, &conn).is_err() {
            rows.push(ProviderTestRow {
                slug: provider.slug,
                label,
                default_model: provider.default_model,
                flavor: provider.flavor,
                status: "no_key".into(),
                detail: format!(
                    "no API key configured (set ${} or add via GUI Settings → API Keys)",
                    provider.env_var
                ),
                duration_ms: None,
            });
            continue;
        }

        // Dispatch the smoke prompt. dispatch_with_history wraps the
        // provider-flavor logic, so success here exercises the same
        // path real user dispatches take — not a separate code path.
        let started = std::time::Instant::now();
        let outcome = dispatch_with_history(provider, &[], SMOKE_PROMPT, None, &conn);
        let duration_ms = started.elapsed().as_millis() as i64;
        match outcome {
            Ok(o) if o.response.is_some() => {
                let resp = o.response.as_deref().unwrap_or("");
                rows.push(ProviderTestRow {
                    slug: provider.slug,
                    label,
                    default_model: provider.default_model,
                    flavor: provider.flavor,
                    status: "ok".into(),
                    detail: format!(
                        "model {} returned {}-char reply",
                        o.model_used,
                        resp.chars().count()
                    ),
                    duration_ms: Some(duration_ms),
                });
            }
            Ok(o) => {
                any_fail = true;
                rows.push(ProviderTestRow {
                    slug: provider.slug,
                    label,
                    default_model: provider.default_model,
                    flavor: provider.flavor,
                    status: "http_error".into(),
                    detail: o
                        .error_message
                        .unwrap_or_else(|| "(no error_message)".into()),
                    duration_ms: Some(duration_ms),
                });
            }
            Err(e) => {
                any_fail = true;
                rows.push(ProviderTestRow {
                    slug: provider.slug,
                    label,
                    default_model: provider.default_model,
                    flavor: provider.flavor,
                    status: "dispatch_error".into(),
                    detail: format!("{}", e),
                    duration_ms: Some(duration_ms),
                });
            }
        }
    }

    if opts.human {
        emit_human(&format!("Provider smoke test — {} provider(s):", rows.len()));
        for r in &rows {
            let tag = match r.status.as_str() {
                "ok" => "✓ ok",
                "no_key" => "—  no key",
                "http_error" => "✗ HTTP error",
                "dispatch_error" => "✗ error",
                _ => "?  unknown",
            };
            let dur = r
                .duration_ms
                .map(|d| format!("  ({}ms)", d))
                .unwrap_or_default();
            emit_human(&format!(
                "  {:14} {:14} model={:24} {}",
                r.slug,
                tag,
                r.default_model,
                dur,
            ));
            emit_human(&format!("                 {}", r.detail));
        }
        if any_fail {
            emit_human("\nOne or more providers failed. Exit code 1.");
        }
    } else {
        emit_json(&rows)?;
    }
    if any_fail {
        std::process::exit(1);
    }
    Ok(())
}
