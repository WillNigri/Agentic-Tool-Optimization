// v2.3.8 Phase 4.2 — Ops recipe execution engine.
// v2.3.11 Phase 4.5 — PostWebhook executor added.
// v2.3.12 Phase 4.6 — RunScript executor added.
//
// Long-running tokio task that:
//   1. Subscribes to events::bus
//   2. For each event, queries ops_recipes for enabled recipes whose
//      trigger_type matches AND whose optional trigger filters match
//      the event payload
//   3. Runs each matching recipe's action
//   4. Audits the run to ops_recipe_runs
//
// Implemented action executors: DraftSkillFromReplay, ReplayOnAlt,
// DispatchAgent, PostWebhook, RunScript. Stubs: KillRun (waits on an
// event variant that carries a live active_runs key), NotifyHuman
// (waits on the Phase 5 activity feed).

use crate::events::{bus, AtoEvent, RegressionSeverity, ReplayStatus};
use crate::recipes::{OpsRecipe, RecipeAction, RecipeTrigger};
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// v2.3.10 — per-recipe rate-limit mutexes. Codex flagged the
/// previous check-then-insert as non-atomic: two concurrent
/// handle_event invocations could both observe count=9 and both fire.
/// Mutex<()> here serializes the "count + decide + insert" sequence
/// per recipe slug. Different recipes lock independently.
fn rate_limit_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_for_slug(slug: &str) -> Arc<Mutex<()>> {
    let mut map = rate_limit_locks().lock().expect("rate-limit map poisoned");
    map.entry(slug.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Start the engine. Spawns TWO tokio tasks:
///   1. Live bus subscriber — fast path for in-process events
///   2. events_log poll loop — picks up cross-process events (CLI
///      dispatches publish there since they can't reach the in-memory
///      bus). Polls every 2s for new event_seqs.
///
/// Both paths converge on handle_event, which dedupes via
/// ops_recipe_runs (skip events already processed). Multiple calls to
/// start() are safe but only one should fire at app boot.
pub fn start() {
    // Live bus subscriber.
    tauri::async_runtime::spawn(async move {
        let mut rx = bus::subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(e) = handle_event(event).await {
                        eprintln!("recipes_engine: handle_event error: {}", e);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    // Subscriber fell behind. The poll loop below
                    // recovers any missed events from events_log on
                    // the next tick.
                    eprintln!(
                        "recipes_engine: bus lagged {} events; poll loop will recover from events_log",
                        skipped
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // Channel is dead — bus dropped. Exit cleanly.
                    return;
                }
            }
        }
    });

    // events_log poll loop (catches CLI-published events + recovers
    // from any bus lag). Bootstrap last_seen_seq from MAX(event_seq)
    // at startup so we don't reprocess history.
    tauri::async_runtime::spawn(async move {
        let mut last_seen_seq: i64 = 0;
        // Initial bootstrap — skip all existing events so first launch
        // doesn't replay your historical event log into the engine.
        if let Ok(conn) = rusqlite::Connection::open(crate::get_db_path()) {
            last_seen_seq = conn
                .query_row("SELECT COALESCE(MAX(event_seq), 0) FROM events_log", [], |r| r.get(0))
                .unwrap_or(0);
        }
        let interval = std::time::Duration::from_millis(2000);
        loop {
            tokio::time::sleep(interval).await;
            match poll_events_log(last_seen_seq) {
                Ok((events, max_seen)) => {
                    last_seen_seq = max_seen.max(last_seen_seq);
                    for ev in events {
                        if let Err(e) = handle_event(ev).await {
                            eprintln!("recipes_engine: poll handle_event error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("recipes_engine: poll error: {}", e);
                }
            }
        }
    });
}

/// Read events_log rows with event_seq > since, in seq order. Returns
/// the parsed events and the highest event_seq seen (or `since` if no
/// new rows). Best-effort: a locked DB returns no rows and the next
/// tick retries.
fn poll_events_log(since: i64) -> Result<(Vec<AtoEvent>, i64), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let mut stmt = conn
        .prepare(
            "SELECT event_seq, payload FROM events_log WHERE event_seq > ?1 ORDER BY event_seq ASC LIMIT 200",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    let mut max_seq = since;
    for (seq, payload) in rows {
        if seq > max_seq {
            max_seq = seq;
        }
        match serde_json::from_str::<AtoEvent>(&payload) {
            Ok(ev) => events.push(ev),
            Err(e) => {
                // Skip malformed rows; advance past them so we don't
                // re-hit on every tick.
                eprintln!("recipes_engine: skip malformed event #{}: {}", seq, e);
            }
        }
    }
    Ok((events, max_seq))
}

async fn handle_event(event: AtoEvent) -> Result<(), String> {
    let event_type = event.type_name();
    let event_payload =
        serde_json::to_string(&event).map_err(|e| format!("serialize event: {}", e))?;

    // v2.3.9 — Recipes store trigger_type with an `on_` prefix
    // ("on_replay_done") while events publish without it
    // ("replay_done"). The mismatch was caught by codex in the 4.1
    // review but not fixed until end-to-end dogfooding surfaced the
    // silent no-fire. Build the recipe-side lookup key explicitly.
    let trigger_lookup = format!("on_{}", event_type);

    // Find all enabled recipes whose trigger_type matches this event.
    let candidates = match find_candidates(&trigger_lookup) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("recipes_engine: candidate lookup failed: {}", e);
            return Err(e);
        }
    };
    if candidates.is_empty() {
        return Ok(());
    }

    for recipe in candidates {
        if !trigger_filters_match(&recipe.trigger, &event) {
            continue;
        }

        // v2.3.10 Phase 4.4 — per-recipe rate limit. Catches infinite
        // recursion (action→event→same recipe) and general runaway
        // recipes. 10 successful/failed runs in any 60s window per
        // recipe. Rate-limited rows are NOT counted toward the next
        // window's quota (codex #3 from the 4.4 review).
        //
        // Atomicity: the check-then-insert is serialized per recipe
        // via a slug-keyed Mutex (codex #2). Different recipes lock
        // independently so unrelated triggers stay parallel.
        let slug_lock = lock_for_slug(&recipe.slug);
        let _serialize = slug_lock.lock().expect("recipe rate-limit lock poisoned");
        if let Some(count) = runs_in_window_executed_only(&recipe.slug, 60) {
            if count >= 10 {
                let run_id = uuid::Uuid::new_v4().to_string();
                let started_at = chrono::Utc::now().to_rfc3339();
                insert_run_row(
                    &run_id,
                    &recipe,
                    event.event_seq() as i64,
                    event_type,
                    &event_payload,
                    &started_at,
                );
                finalize_run_row(
                    &run_id,
                    ActionOutcome {
                        status: "rate_limited",
                        result: None,
                        error: Some(format!(
                            "recipe @{} hit the rate limit ({} executed runs in the last 60s)",
                            recipe.slug, count
                        )),
                    },
                    &started_at,
                );
                // Drop the lock before continuing.
                drop(_serialize);
                continue;
            }
        }

        // Insert the audit row in 'running' state BEFORE releasing the
        // lock — that way concurrent invocations see this run in
        // runs_in_window_executed_only and back off correctly.
        let run_id = uuid::Uuid::new_v4().to_string();
        let started_at = chrono::Utc::now().to_rfc3339();
        insert_run_row(
            &run_id,
            &recipe,
            event.event_seq() as i64,
            event_type,
            &event_payload,
            &started_at,
        );
        drop(_serialize);

        // Execute the action.
        let outcome = execute_action(&recipe.action, &event).await;
        let finished_at = chrono::Utc::now().to_rfc3339();
        finalize_run_row(&run_id, outcome, &finished_at);
    }
    Ok(())
}

/// Count ops_recipe_runs rows for `slug` in the last `window_secs`
/// seconds, EXCLUDING rate_limited rows (so blocked attempts don't
/// extend the block window — codex #3 from the 4.4 review).
fn runs_in_window_executed_only(slug: &str, window_secs: i64) -> Option<i64> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let cutoff =
        (chrono::Utc::now() - chrono::Duration::seconds(window_secs)).to_rfc3339();
    conn.query_row(
        "SELECT COUNT(*) FROM ops_recipe_runs WHERE recipe_slug = ?1 AND started_at > ?2 AND status != 'rate_limited'",
        rusqlite::params![slug, cutoff],
        |r| r.get::<_, i64>(0),
    )
    .ok()
}


fn find_candidates(event_type: &str) -> Result<Vec<OpsRecipe>, String> {
    let db_path = crate::get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let mut stmt = conn.prepare(
        "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
           FROM ops_recipes WHERE trigger_type = ?1 AND enabled = 1",
    ).map_err(|e| e.to_string())?;
    let rows: Vec<(String, String, String, Option<String>, String, String, i64, String, String)> = stmt
        .query_map([event_type], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, slug, name, description, tj, aj, enabled_int, created_at, updated_at) in rows {
        let trigger: RecipeTrigger = serde_json::from_str(&tj).map_err(|e| e.to_string())?;
        let action: RecipeAction = serde_json::from_str(&aj).map_err(|e| e.to_string())?;
        out.push(OpsRecipe {
            id,
            slug,
            name,
            description,
            trigger,
            action,
            enabled: enabled_int != 0,
            created_at,
            updated_at,
        });
    }
    Ok(out)
}

/// Apply optional trigger filters. None values mean "match any."
fn trigger_filters_match(trigger: &RecipeTrigger, event: &AtoEvent) -> bool {
    match (trigger, event) {
        (
            RecipeTrigger::OnRegressionDetected {
                severity: tsev,
                agent_slug: tslug,
            },
            AtoEvent::RegressionDetected {
                severity: esev,
                agent_slug: easlug,
                ..
            },
        ) => {
            if let Some(want) = tsev {
                let got = match esev {
                    RegressionSeverity::Regression => "regression",
                    RegressionSeverity::Improvement => "improvement",
                };
                if want != got {
                    return false;
                }
            }
            if let Some(want) = tslug {
                if want != easlug {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnDispatchFailed {
                runtime: trt,
                agent_slug: tslug,
            },
            AtoEvent::DispatchFailed {
                runtime: ert,
                agent_slug: easlug,
                ..
            },
        ) => {
            if let Some(want) = trt {
                if want != ert {
                    return false;
                }
            }
            if let Some(want) = tslug {
                match easlug {
                    Some(got) if want == got => (),
                    _ => return false,
                }
            }
            true
        }
        (
            RecipeTrigger::OnReplayDone {
                status: tstatus,
                target_runtime: trt,
            },
            AtoEvent::ReplayDone {
                status: estatus,
                target_runtime: ert,
                ..
            },
        ) => {
            if let Some(want) = tstatus {
                let got = match estatus {
                    ReplayStatus::Done => "done",
                    ReplayStatus::Failed => "failed",
                };
                if want != got {
                    return false;
                }
            }
            if let Some(want) = trt {
                if want != ert {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnCostThresholdExceeded {
                window: twin,
                agent_slug: tslug,
            },
            AtoEvent::CostThresholdExceeded {
                agent_slug: easlug,
                ..
            },
        ) => {
            // window enum-to-string serialization not yet exposed
            // cleanly; for v1 we only filter on agent_slug.
            let _ = twin;
            if let Some(want) = tslug {
                if want != easlug {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnSchedule {
                cron: tcron,
                agent_slug: tslug,
            },
            AtoEvent::ScheduleFired {
                cron_id: ecron,
                agent_slug: easlug,
                ..
            },
        ) => {
            if let Some(want) = tcron {
                if want != ecron {
                    return false;
                }
            }
            if let Some(want) = tslug {
                if want != easlug {
                    return false;
                }
            }
            true
        }
        _ => false, // trigger type mismatch (shouldn't reach here given the SQL filter)
    }
}

// ─── Action executors ──────────────────────────────────────────────────

struct ActionOutcome {
    status: &'static str, // "done" | "failed" | "not_implemented"
    result: Option<String>,
    error: Option<String>,
}

async fn execute_action(action: &RecipeAction, event: &AtoEvent) -> ActionOutcome {
    match action {
        RecipeAction::DraftSkillFromReplay { out } => draft_skill_from_replay(event, out.as_deref()),
        RecipeAction::ReplayOnAlt {
            target_runtime,
            target_model,
        } => {
            // Substitute simple placeholders. Phase 4.2 only supports
            // {{source_runtime}} — the auto-replay template's needs.
            let resolved = substitute_simple_placeholders(target_runtime, event);
            replay_on_alt(event, &resolved, target_model.as_deref()).await
        }
        // v2.3.10 Phase 4.4 — KillRun + DispatchAgent now implemented.
        RecipeAction::KillRun => kill_run(event),
        RecipeAction::DispatchAgent {
            runtime,
            agent_slug,
            prompt_template,
        } => {
            let resolved_runtime = substitute_simple_placeholders(runtime, event);
            let resolved_prompt = substitute_simple_placeholders(prompt_template, event);
            dispatch_agent(&resolved_runtime, agent_slug.as_deref(), &resolved_prompt).await
        }
        // v2.3.11 Phase 4.5 — PostWebhook implemented.
        RecipeAction::PostWebhook { url, body_template } => {
            post_webhook(event, url, body_template.as_deref()).await
        }
        // v2.3.12 Phase 4.6 — RunScript implemented.
        RecipeAction::RunScript { path, args } => run_script(event, path, args).await,
        // Phase 4.6 v1 stub — NotifyHuman waits for Phase 5 activity feed.
        RecipeAction::NotifyHuman { .. } => ActionOutcome {
            status: "not_implemented",
            result: None,
            error: Some(format!(
                "Action '{}' is not yet implemented.",
                action_name(action)
            )),
        },
    }
}

/// Known {{placeholder}} tokens. Used for two purposes:
///   1. Detect which placeholders a URL/body template uses, so we can
///      validate the event actually carries values for them BEFORE
///      substitution (codex round-2 caught that substituting empty
///      strings silently hid missing fields).
///   2. Drive the per-token replace loop in apply_substitution.
const KNOWN_PLACEHOLDERS: &[&str] = &[
    "{{source_runtime}}",
    "{{target_runtime}}",
    "{{agent_slug}}",
    "{{previous_runtime}}",
];

#[derive(Default)]
struct EventFields {
    source_runtime: String,
    target_runtime: String,
    agent_slug: String,
    previous_runtime: String,
}

impl EventFields {
    fn lookup(&self, placeholder: &str) -> &str {
        match placeholder {
            "{{source_runtime}}" => &self.source_runtime,
            "{{target_runtime}}" => &self.target_runtime,
            "{{agent_slug}}" => &self.agent_slug,
            "{{previous_runtime}}" => &self.previous_runtime,
            _ => "",
        }
    }
}

fn extract_event_fields(event: &AtoEvent) -> EventFields {
    match event {
        AtoEvent::RegressionDetected {
            agent_slug,
            field,
            old_value,
            new_value,
            ..
        } => {
            let prev = if field == "runtime" {
                old_value.clone().unwrap_or_default()
            } else {
                String::new()
            };
            let curr = if field == "runtime" {
                new_value.clone().unwrap_or_default()
            } else {
                String::new()
            };
            EventFields {
                source_runtime: curr,
                target_runtime: prev.clone(),
                agent_slug: agent_slug.clone(),
                previous_runtime: prev,
            }
        }
        AtoEvent::ReplayDone {
            source_runtime,
            target_runtime,
            ..
        } => EventFields {
            source_runtime: source_runtime.clone(),
            target_runtime: target_runtime.clone(),
            agent_slug: String::new(),
            previous_runtime: source_runtime.clone(),
        },
        AtoEvent::DispatchFailed {
            runtime,
            agent_slug,
            ..
        } => EventFields {
            source_runtime: runtime.clone(),
            target_runtime: String::new(),
            agent_slug: agent_slug.clone().unwrap_or_default(),
            previous_runtime: String::new(),
        },
        _ => EventFields::default(),
    }
}

/// Return the first placeholder used in `template` whose value in the
/// event is empty (missing field). Codex round-2: previously we tried
/// to detect unresolved placeholders AFTER substitution, but
/// substitute_simple_placeholders replaces unknown fields with "" so
/// the literal placeholder was never visible in the output. The right
/// time to check is BEFORE substitution, against the template + the
/// event's actual field values.
fn first_missing_placeholder(
    template: &str,
    fields: &EventFields,
) -> Option<&'static str> {
    for ph in KNOWN_PLACEHOLDERS {
        if template.contains(ph) && fields.lookup(ph).is_empty() {
            return Some(ph);
        }
    }
    None
}

/// Redact a webhook URL for audit logs. Slack/Discord URLs are
/// credentials (anyone holding the URL can post to that channel).
/// We keep scheme + host (+ port if non-default) and drop the
/// path/query/fragment. IPv6 hosts get re-bracketed since `host_str()`
/// returns them unbracketed.
fn redact_url(url: &str) -> String {
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return "[unparseable URL]".to_string(),
    };
    let mut out = format!("{}://", parsed.scheme());
    match parsed.host_str() {
        Some(h) if h.contains(':') => {
            out.push('[');
            out.push_str(h);
            out.push(']');
        }
        Some(h) => out.push_str(h),
        None => out.push('?'),
    }
    if let Some(port) = parsed.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str("/…");
    out
}

/// JSON-escape a string for safe inline substitution inside a JSON body
/// template (template author writes `"name": "{{source_runtime}}"`).
/// Uses serde_json::to_string for correctness and strips outer quotes
/// since the template already provides them.
fn json_escape_inner(s: &str) -> String {
    let encoded = match serde_json::to_string(s) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    if encoded.len() >= 2 && encoded.starts_with('"') && encoded.ends_with('"') {
        encoded[1..encoded.len() - 1].to_string()
    } else {
        encoded
    }
}

/// Single-pass placeholder substitution.
///
/// Codex round-3 caught that the previous implementation
/// (`out = out.replace(ph, ...)` looped over each known placeholder)
/// was order-dependent and could re-expand placeholder-shaped content
/// from a substituted value. E.g. agent_slug = "{{previous_runtime}}"
/// would get its inner placeholder expanded on the next loop iteration.
///
/// This walks the template left-to-right, emits non-placeholder text
/// verbatim, and resolves `{{known_token}}` ranges once each. Unknown
/// `{{...}}` tokens are passed through unchanged (intentional — users
/// may template-process downstream like Slack's own `{user_id}`).
fn apply_substitution(template: &str, fields: &EventFields, json_safe: bool) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while !rest.is_empty() {
        let Some(open_idx) = rest.find("{{") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..open_idx]);
        let after_open = &rest[open_idx + 2..];
        let Some(close_rel) = after_open.find("}}") else {
            // Unmatched "{{" — emit the rest verbatim.
            out.push_str(&rest[open_idx..]);
            break;
        };
        let token_end = open_idx + 2 + close_rel + 2;
        let token = &rest[open_idx..token_end];
        if KNOWN_PLACEHOLDERS.contains(&token) {
            let value = fields.lookup(token);
            if json_safe {
                out.push_str(&json_escape_inner(value));
            } else {
                out.push_str(value);
            }
        } else {
            // Unknown placeholder — keep verbatim.
            out.push_str(token);
        }
        rest = &rest[token_end..];
    }
    out
}

/// Executor: POST the event payload to a user-supplied URL.
///
/// Use cases: Slack incoming webhooks ("@channel a regression just
/// fired"), Discord webhooks, custom dashboards.
///
/// Security posture for v1 (post codex review):
///   - URL parsed via `reqwest::Url::parse` (not just a prefix check).
///     Scheme must be http or https. Rejects file://, javascript:,
///     data:, gopher:, and malformed URLs.
///   - URL is NOT screened for private/internal IPs (SSRF). Recipes are
///     user-authored in v1, so the user owns the destination. If/when
///     recipes get imported from a marketplace, this is where the
///     allowlist policy lands. HTTP redirects are NOT disabled, so the
///     SSRF surface includes anywhere reqwest follows redirects to —
///     documenting that explicitly per codex feedback.
///   - 10s timeout. Webhooks should be fast.
///   - Content-Type is always application/json. body_template values
///     are JSON-escaped on substitution so a `"` or newline in an agent
///     slug can't corrupt the JSON shape.
///   - Audit logs (ops_recipe_runs.result / .error) NEVER contain the
///     full URL — only scheme+host. Webhook URLs are credentials and
///     leaking them to disk would be a real secret-exposure.
///   - Unresolved KNOWN placeholders in URL/body fail loud. The check
///     is precise (matches `{{source_runtime}}` etc., not any `{{`) so
///     user templates that legitimately contain `{{` for unrelated
///     reasons aren't false-flagged.
async fn post_webhook(
    event: &AtoEvent,
    url: &str,
    body_template: Option<&str>,
) -> ActionOutcome {
    let fields = extract_event_fields(event);

    if let Some(p) = first_missing_placeholder(url, &fields) {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "post_webhook: URL uses placeholder {} but event did not provide a value",
                p
            )),
        };
    }
    let url_resolved = apply_substitution(url, &fields, false);
    if url_resolved.is_empty() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some("post_webhook: empty URL after substitution".to_string()),
        };
    }
    let parsed_url = match reqwest::Url::parse(&url_resolved) {
        Ok(u) => u,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("post_webhook: invalid URL: {}", e)),
            };
        }
    };
    let scheme = parsed_url.scheme();
    if scheme != "http" && scheme != "https" {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "post_webhook: scheme must be http or https, got '{}'",
                scheme
            )),
        };
    }
    let redacted = redact_url(&url_resolved);
    let body = match body_template {
        Some(t) => {
            if let Some(p) = first_missing_placeholder(t, &fields) {
                return ActionOutcome {
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "post_webhook: body uses placeholder {} but event did not provide a value",
                        p
                    )),
                };
            }
            let resolved = apply_substitution(t, &fields, true);
            // Validate the resolved body is parseable JSON. Codex
            // round-2: a malformed template like `"k": {{x}}` (no quotes
            // around the placeholder) would produce invalid JSON post-
            // substitution. Catch it here, not at the remote endpoint.
            if let Err(e) =
                serde_json::from_str::<serde_json::Value>(&resolved)
            {
                return ActionOutcome {
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "post_webhook: body is not valid JSON after substitution: {}",
                        e
                    )),
                };
            }
            resolved
        }
        None => match serde_json::to_string(event) {
            Ok(s) => s,
            Err(e) => {
                return ActionOutcome {
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "post_webhook: serialize event failed: {}",
                        e
                    )),
                };
            }
        },
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("post_webhook: build client: {}", e)),
            };
        }
    };
    match client
        .post(parsed_url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(resp) => {
            let status_code = resp.status();
            if status_code.is_success() {
                ActionOutcome {
                    status: "done",
                    result: Some(format!("POST {} → {}", redacted, status_code)),
                    error: None,
                }
            } else {
                ActionOutcome {
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "post_webhook: {} returned {}",
                        redacted, status_code
                    )),
                }
            }
        }
        Err(e) => ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!("post_webhook: {} → {}", redacted, e)),
        },
    }
}

/// Read from `reader` until EOF or read error, keeping at most
/// `keep_cap` bytes in the returned buffer. Bytes beyond `keep_cap`
/// are drained-and-discarded so the child's pipe stays unblocked
/// (preventing the child from stalling on a full pipe and never
/// exiting). Returns (kept_bytes, total_bytes_seen).
///
/// Codex round-1 4.6: an unbounded wait_with_output() can OOM the
/// desktop on a noisy script before our 30s timeout fires.
async fn read_capped<R>(mut reader: R, keep_cap: usize) -> (Vec<u8>, usize)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::with_capacity(keep_cap.min(4096));
    let mut total = 0usize;
    let mut tmp = [0u8; 4096];
    loop {
        match reader.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => {
                total = total.saturating_add(n);
                if kept.len() < keep_cap {
                    let remaining = keep_cap - kept.len();
                    let take = remaining.min(n);
                    kept.extend_from_slice(&tmp[..take]);
                }
            }
            Err(_) => break,
        }
    }
    (kept, total)
}

/// Truncate `s` to at most `max_chars` chars, appending an ellipsis
/// if truncation happened.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// Await a reader task to completion (post-success). Returns
/// (kept_bytes, total_bytes, join_status). join_status is None on
/// normal completion; Some("panicked") / Some("cancelled") on a
/// JoinError so the audit can surface lost output rather than
/// silently emit empty.
async fn drain_reader(
    handle: Option<tokio::task::JoinHandle<(Vec<u8>, usize)>>,
) -> (Vec<u8>, usize, Option<&'static str>) {
    match handle {
        Some(h) => match h.await {
            Ok(v) => (v.0, v.1, None),
            Err(e) if e.is_panic() => (Vec::new(), 0, Some("panicked")),
            Err(_) => (Vec::new(), 0, Some("cancelled")),
        },
        None => (Vec::new(), 0, None),
    }
}

/// Abort a reader task and await its cancellation so it doesn't run
/// detached after the parent has given up on the child.
async fn abort_reader(
    handle: Option<tokio::task::JoinHandle<(Vec<u8>, usize)>>,
) {
    if let Some(h) = handle {
        h.abort();
        let _ = h.await;
    }
}

/// Executor: run a local script.
///
/// The script's exit code is the source of truth for action success:
/// exit 0 → done, non-zero → failed. The event JSON is offered on
/// stdin as a best-effort delivery (see "stdin contract" below).
///
/// Use cases: arbitrary glue — write to a log file, run `say` on macOS,
/// trigger an external CLI tool, ping a teammate via `osascript`.
///
/// Stdin contract (codex rounds 3-4): scripts may or may not read
/// stdin. We attempt delivery with a 5s timeout. The outcome ("ok",
/// "broken-pipe", "timed-out", "io-error(...)", "no-handle") is
/// captured into stdin_summary and surfaced in the audit, but does
/// NOT fail the action — many useful scripts ignore stdin entirely
/// (e.g. `osascript -e ...`, `say "hello"`, scripts that only need
/// argv). The payload is the JSON-serialized event followed by a
/// trailing newline (so bash `read` returns success on delivered
/// input). If your script DEPENDS on the event JSON, read it and
/// exit non-zero when empty:
///   `read -t 1 line || exit 1` (bash, works because of the \n).
///
/// Security posture for v1 (post codex round-1 review):
///   - Path MUST be absolute. Rejects relative paths so the engine's
///     CWD (desktop process working dir) is never load-bearing.
///   - Path is canonicalized via std::fs::canonicalize so "." and
///     ".." segments resolve before execution. Symlinks ARE followed
///     — recipes are user-authored, the user owns the destination.
///   - Canonical target must be a regular file with at least one exec
///     bit (Unix). Windows trusts the file extension.
///   - Spawned via tokio::process::Command (no shell). Args are
///     individual Vec<String> entries via .args(), no shell interp.
///   - env_clear() + only PATH inherited. The script can't read API
///     keys / secrets from the desktop process env.
///   - Working directory is the SCRIPT'S PARENT directory (codex
///     round-1 nit: `$HOME` was a surprising default since it has
///     nothing to do with where the script lives). Falls back to home
///     if the canonical path has no parent (effectively impossible
///     for a regular file).
///   - Three timeouts:
///       * 5s on the stdin write (codex round-1 #2: scripts that
///         never read stdin could otherwise hang us waiting for the
///         pipe buffer to drain).
///       * 30s hard wait on the child. On expiry: start_kill →
///         wait → return failed. Explicit kill, not just kill_on_drop
///         (codex round-1 nit: confirms-and-reaps instead of dropping
///         to GC). kill_on_drop is still set as a safety net.
///       * Reader tasks read until EOF or error — bounded since the
///         child WILL exit (on success, or via the explicit kill on
///         timeout).
///   - Bounded readers cap kept bytes at 8000 (stdout) / 2000 (stderr).
///     Reads beyond the cap are drained-and-discarded so the child
///     never stalls on a full pipe. Total byte counts are kept so the
///     audit reflects the script's actual output volume.
///   - Audit log persists only a TRUNCATED excerpt (800 chars stdout,
///     400 stderr) plus byte counts and exit code. Codex round-1 #3:
///     scripts can echo secrets to stdout; durably storing the full
///     stream in ops_recipe_runs would be a privacy foot-gun. Scripts
///     that want to keep full output should write to their own file.
///
/// Acceptable threat-model gap: a script CAN do anything the user can
/// (rm -rf, curl secrets out, etc.). Recipes are user-authored in v1
/// so this matches the SSRF posture from PostWebhook — the user owns
/// what they invoke. Imported/marketplace recipes will need an
/// allowlist before this executor is safe in that flow.
async fn run_script(
    event: &AtoEvent,
    path: &str,
    args: &[String],
) -> ActionOutcome {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "run_script: path must be absolute, got '{}'",
                path
            )),
        };
    }
    let canonical = match std::fs::canonicalize(p) {
        Ok(c) => c,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: cannot resolve path '{}': {}",
                    path, e
                )),
            };
        }
    };
    let meta = match std::fs::metadata(&canonical) {
        Ok(m) => m,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: cannot stat '{}': {}",
                    canonical.display(),
                    e
                )),
            };
        }
    };
    if !meta.is_file() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "run_script: not a regular file: {}",
                canonical.display()
            )),
        };
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: not executable (no x bit): {}",
                    canonical.display()
                )),
            };
        }
    }

    let event_json = match serde_json::to_string(event) {
        Ok(mut s) => {
            // Append trailing newline so scripts using `read` get a
            // complete line (codex round-4: `read -t 1 line || exit 1`
            // would return non-zero on EOF without newline even when
            // bytes were delivered). JSON parsers (jq, python, etc.)
            // are tolerant of trailing whitespace.
            s.push('\n');
            s
        }
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: serialize event failed: {}",
                    e
                )),
            };
        }
    };

    let path_env = std::env::var("PATH").unwrap_or_default();
    let cwd = canonical
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(crate::home_dir);

    let mut cmd = tokio::process::Command::new(&canonical);
    cmd.args(args)
        .current_dir(&cwd)
        .env_clear()
        .env("PATH", path_env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: spawn '{}': {}",
                    canonical.display(),
                    e
                )),
            };
        }
    };

    // Take the pipe handles so we can spawn reader tasks while the
    // child runs. wait() on its own doesn't drain pipes — combined
    // with bounded readers this prevents OOM and pipe-stall hangs.
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    const STDOUT_KEEP: usize = 8000;
    const STDERR_KEEP: usize = 2000;
    let stdout_task = stdout.map(|s| tokio::spawn(read_capped(s, STDOUT_KEEP)));
    let stderr_task = stderr.map(|s| tokio::spawn(read_capped(s, STDERR_KEEP)));

    // Bound the stdin write — if the script never reads stdin and
    // we're writing more than the pipe buffer (~64KB), write_all
    // would hang. AtoEvent serializations are tiny today, but 5s is
    // cheap insurance against future event-payload growth.
    //
    // Codex round-2 #1: capture the outcome rather than discarding.
    // Silent failure could let a partial/dropped stdin write produce
    // a "done" recipe run, hiding a real delivery gap.
    let stdin_summary: String = match stdin {
        Some(mut s) => {
            use tokio::io::AsyncWriteExt;
            let outcome = tokio::time::timeout(
                Duration::from_secs(5),
                s.write_all(event_json.as_bytes()),
            )
            .await;
            // s dropped at end of this arm → pipe closes, EOF to child.
            match outcome {
                Ok(Ok(())) => "ok".to_string(),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                    // Script closed stdin before we finished writing.
                    // Expected for scripts that don't read input.
                    "broken-pipe".to_string()
                }
                Ok(Err(e)) => format!("io-error({})", e),
                Err(_) => "timed-out".to_string(),
            }
        }
        None => "no-handle".to_string(),
    };

    // Wait for the child with a hard timeout. On expiry: explicit
    // start_kill + wait to confirm reap (not just dropping the
    // future for kill_on_drop to handle).
    let wait_result =
        tokio::time::timeout(Duration::from_secs(30), child.wait()).await;
    let exit_status = match wait_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            // Codex round-2 nit: mirror the timeout-path reader
            // cleanup here so detached tasks don't keep running after
            // we've already given up on the child.
            abort_reader(stdout_task).await;
            abort_reader(stderr_task).await;
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: wait '{}': {} (stdin={})",
                    canonical.display(),
                    e,
                    stdin_summary,
                )),
            };
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            abort_reader(stdout_task).await;
            abort_reader(stderr_task).await;
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!(
                    "run_script: '{}' exceeded 30s timeout — killed (stdin={})",
                    canonical.display(),
                    stdin_summary,
                )),
            };
        }
    };

    // Process exited cleanly; collect reader results.
    let (stdout_kept, stdout_total, stdout_join) = drain_reader(stdout_task).await;
    let (stderr_kept, stderr_total, stderr_join) = drain_reader(stderr_task).await;

    const STDOUT_AUDIT_CHARS: usize = 800;
    const STDERR_AUDIT_CHARS: usize = 400;
    let stdout_audit = truncate_chars(
        &String::from_utf8_lossy(&stdout_kept),
        STDOUT_AUDIT_CHARS,
    );
    let stderr_audit = truncate_chars(
        &String::from_utf8_lossy(&stderr_kept),
        STDERR_AUDIT_CHARS,
    );

    let stdout_label = stdout_join.map(|s| format!(" [reader:{}]", s)).unwrap_or_default();
    let stderr_label = stderr_join.map(|s| format!(" [reader:{}]", s)).unwrap_or_default();

    if exit_status.success() {
        ActionOutcome {
            status: "done",
            result: Some(format!(
                "exit=0 stdin={} stdout_bytes={}{} stderr_bytes={}{} stdout='{}' stderr='{}'",
                stdin_summary,
                stdout_total,
                stdout_label,
                stderr_total,
                stderr_label,
                if stdout_audit.is_empty() {
                    "(empty)"
                } else {
                    &stdout_audit
                },
                if stderr_audit.is_empty() {
                    "(empty)"
                } else {
                    &stderr_audit
                },
            )),
            error: None,
        }
    } else {
        ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "run_script: '{}' exited with {:?} (stdin={}, stdout_bytes={}{}, stderr_bytes={}{}){}",
                canonical.display(),
                exit_status.code(),
                stdin_summary,
                stdout_total,
                stdout_label,
                stderr_total,
                stderr_label,
                if stderr_audit.is_empty() {
                    String::new()
                } else {
                    format!(" — stderr: {}", stderr_audit)
                }
            )),
        }
    }
}

/// Executor: kill a run referenced by the trigger event's payload.
///
/// Honest scope (caught by codex in v2.3.10 review): in v1, the events
/// that carry an ID DON'T carry a live active_runs registry key — they
/// carry execution_logs.id (DispatchFailed fires AFTER the run has
/// already finished, so there's nothing live to kill). So this executor
/// will essentially always be a no-op until a future event type
/// (e.g. DispatchTimedOut, or a periodic "long-running" tick) carries
/// the live registry ID.
///
/// We return status="not_implemented_yet" rather than fake "done" to
/// surface the limitation visibly. Reinstate full behavior when an
/// event variant ships with an actionable live-registry key.
fn kill_run(event: &AtoEvent) -> ActionOutcome {
    ActionOutcome {
        status: "not_implemented_yet",
        result: None,
        error: Some(format!(
            "kill_run can't act on '{}' — the event carries execution_logs.id, not the live active_runs registry key. The action will work once an event variant ships with a live run handle (planned for a future 'on_dispatch_long_running' trigger).",
            event.type_name()
        )),
    }
}

/// Executor: dispatch a prompt to an agent on a runtime. Useful for
/// reactive workflows — "when a regression fires, ask @triage to
/// investigate." Uses prompt_agent_inner (same path the GUI uses),
/// not the CLI dispatch — so the new run shows up in Live Runs
/// immediately.
async fn dispatch_agent(
    runtime: &str,
    agent_slug: Option<&str>,
    prompt: &str,
) -> ActionOutcome {
    if runtime.is_empty() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some("dispatch_agent: empty runtime after substitution".to_string()),
        };
    }
    if prompt.is_empty() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some("dispatch_agent: empty prompt after substitution".to_string()),
        };
    }
    match crate::prompt_agent(
        runtime.to_string(),
        prompt.to_string(),
        None,                                  // config / model override unused for v1
        agent_slug.map(|s| s.to_string()),
        None,                                  // workspace unused
    )
    .await
    {
        Ok(response) => ActionOutcome {
            status: "done",
            result: Some(format!(
                "Dispatched to {}: {} chars response",
                runtime,
                response.len()
            )),
            error: None,
        },
        Err(e) => ActionOutcome {
            status: "failed",
            result: None,
            error: Some(e),
        },
    }
}

fn action_name(a: &RecipeAction) -> &'static str {
    match a {
        RecipeAction::DraftSkillFromReplay { .. } => "draft_skill_from_replay",
        RecipeAction::ReplayOnAlt { .. } => "replay_on_alt",
        RecipeAction::KillRun => "kill_run",
        RecipeAction::DispatchAgent { .. } => "dispatch_agent",
        RecipeAction::PostWebhook { .. } => "post_webhook",
        RecipeAction::NotifyHuman { .. } => "notify_human",
        RecipeAction::RunScript { .. } => "run_script",
    }
}

/// Minimal {{name}} substitution. v1 supports source_runtime,
/// target_runtime, agent_slug. {{previous_runtime}} only resolves for
/// ReplayDone events — RegressionDetected doesn't carry old/new
/// values in its payload yet (Phase 4.3 will add them). Until then,
/// auto-replay-regression-failures template won't fully execute;
/// the action executor's unresolved-placeholder guard surfaces this
/// as a "failed" run with a clear error.
fn substitute_simple_placeholders(template: &str, event: &AtoEvent) -> String {
    let (source_runtime, target_runtime, agent_slug, previous_runtime) = match event {
        AtoEvent::RegressionDetected {
            agent_slug,
            field,
            old_value,
            new_value,
            ..
        } => {
            // For a runtime swap regression, old_value is the previous
            // runtime. v2.3.9 — the schema now carries this.
            let prev = if field == "runtime" {
                old_value.clone().unwrap_or_default()
            } else {
                String::new()
            };
            let curr = if field == "runtime" {
                new_value.clone().unwrap_or_default()
            } else {
                String::new()
            };
            (curr, prev.clone(), agent_slug.clone(), prev)
        }
        AtoEvent::ReplayDone {
            source_runtime,
            target_runtime,
            ..
        } => (
            source_runtime.clone(),
            target_runtime.clone(),
            String::new(),
            source_runtime.clone(),
        ),
        AtoEvent::DispatchFailed {
            runtime,
            agent_slug,
            ..
        } => (
            runtime.clone(),
            String::new(),
            agent_slug.clone().unwrap_or_default(),
            String::new(),
        ),
        _ => (String::new(), String::new(), String::new(), String::new()),
    };
    template
        .replace("{{source_runtime}}", &source_runtime)
        .replace("{{target_runtime}}", &target_runtime)
        .replace("{{agent_slug}}", &agent_slug)
        .replace("{{previous_runtime}}", &previous_runtime)
}

/// Executor: draft a SKILL.md from the trigger's ReplayDone payload.
/// Skillify in action.
fn draft_skill_from_replay(event: &AtoEvent, out_override: Option<&str>) -> ActionOutcome {
    let job_id = match event {
        AtoEvent::ReplayDone { job_id, .. } => job_id.clone(),
        _ => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(
                    "draft_skill_from_replay requires a ReplayDone event".to_string(),
                ),
            };
        }
    };

    // Open DB, read the replay row, render + write skill. Mirrors the
    // CLI's `ato skills draft --from-replay` logic but inlined here so
    // the engine doesn't shell out.
    let db_path = crate::get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("open DB: {}", e)),
            };
        }
    };
    // Tuple type matches the SELECT column order:
    // 0=id String, 1=source_runtime String, 2=target_model Option<String>,
    // 3=target_runtime String, 4=status String, 5=source_prompt Option<String>.
    let row: Result<(String, String, Option<String>, String, String, Option<String>), rusqlite::Error> =
        conn.query_row(
            "SELECT id, source_runtime, target_model, target_runtime, status,
                    (SELECT prompt FROM execution_logs WHERE id = rj.source_execution_log_id) AS source_prompt
               FROM replay_jobs rj WHERE id = ?1",
            [&job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        );
    let (_id, source_runtime, target_model, target_runtime, status, source_prompt) = match row {
        Ok(t) => t,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("replay_jobs row not found: {}", e)),
            };
        }
    };
    if status != "done" {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!("replay job status is '{}', not 'done'", status)),
        };
    }
    let source_prompt = match source_prompt {
        Some(p) if !p.is_empty() => p,
        _ => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some("source prompt missing for replay job".to_string()),
            };
        }
    };
    let skill_name = format!("route-{}-to-{}", source_runtime, target_runtime);
    let prompt_summary = summarize(&source_prompt, 80);
    let body = render_skill_md(
        &skill_name,
        &source_runtime,
        &target_runtime,
        target_model.as_deref(),
        &prompt_summary,
        &job_id,
    );
    let path = match out_override {
        Some(p) => std::path::PathBuf::from(p),
        None => default_skill_path(&target_runtime, &skill_name),
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("mkdir {}: {}", parent.display(), e)),
            };
        }
    }
    if let Err(e) = std::fs::write(&path, body) {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!("write {}: {}", path.display(), e)),
        };
    }
    ActionOutcome {
        status: "done",
        result: Some(format!(
            "Drafted skill '{}' at {}",
            skill_name,
            path.display()
        )),
        error: None,
    }
}

/// Executor: replay the source trace on an alternative runtime.
async fn replay_on_alt(
    event: &AtoEvent,
    target_runtime: &str,
    target_model: Option<&str>,
) -> ActionOutcome {
    if target_runtime.is_empty() || target_runtime.contains("{{") {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "target_runtime unresolved or empty after substitution: '{}'",
                target_runtime
            )),
        };
    }
    // KNOWN GAP (codex-reviewer v2.3.8): the desktop's `start_replay`
    // only resolves source rows by `execution_logs.cloud_trace_id`,
    // but DispatchFailed carries `run_id` which is `execution_logs.id`
    // (no cloud trace yet). Until start_replay accepts execution_logs.id
    // directly, the dispatch_failed → replay_on_alt chain will fail
    // with "prompt-not-local". RegressionDetected's failing_trace_ids
    // ARE cloud trace IDs, so that path works. v1 ships with only the
    // ReplayDone → DraftSkillFromReplay loop fully wired; this path
    // is documented but not yet usable end-to-end.
    let source_trace_id = match event {
        AtoEvent::RegressionDetected {
            failing_trace_ids, ..
        } => failing_trace_ids.first().cloned().unwrap_or_default(),
        AtoEvent::DispatchFailed { run_id, .. } => run_id.clone(),
        _ => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(
                    "replay_on_alt requires RegressionDetected or DispatchFailed".to_string(),
                ),
            };
        }
    };
    if source_trace_id.is_empty() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some("no failing trace id available to replay".to_string()),
        };
    }
    // Delegate to the existing start_replay Tauri command's underlying
    // logic. For Phase 4.2, we just invoke it via its public path —
    // future refactor: extract to a non-Tauri helper so the engine
    // doesn't depend on the Tauri command macros.
    match crate::start_replay(
        source_trace_id.clone(),
        target_runtime.to_string(),
        target_model.map(String::from),
    )
    .await
    {
        Ok(job_id) => ActionOutcome {
            status: "done",
            result: Some(format!("Started replay {}", job_id)),
            error: None,
        },
        Err(e) => ActionOutcome {
            status: "failed",
            result: None,
            error: Some(e),
        },
    }
}

// ─── Helpers (mirror of CLI skills::draft) ────────────────────────────

fn default_skill_path(target_runtime: &str, skill_name: &str) -> std::path::PathBuf {
    let mut home = crate::home_dir();
    match target_runtime {
        "claude" => home.push(".claude/skills"),
        "codex" => home.push(".codex/skills"),
        "gemini" => home.push(".gemini/skills"),
        "openclaw" => home.push(".openclaw/skills"),
        "hermes" => home.push(".hermes/skills"),
        _ => home.push(".ato/skills"),
    }
    home.push(skill_name);
    home.push("SKILL.md");
    home
}

fn render_skill_md(
    name: &str,
    source_runtime: &str,
    target_runtime: &str,
    target_model: Option<&str>,
    prompt_summary: &str,
    job_id: &str,
) -> String {
    let model_line = target_model
        .map(|m| format!("\n# Pinned model: {}\n", m))
        .unwrap_or_default();
    format!(
        "---\nname: {name}\ndescription: \"Route prompts like '{prompt_summary}' to {target_runtime} — earlier replay showed {source_runtime} was failing on this shape.\"\nallowed-tools: []\n---\n{model_line}\n# Why this skill exists\n\nA replay of a real failing dispatch on `{source_runtime}` showed `{target_runtime}` handled the same prompt cleanly. This skill encodes that routing decision so future prompts matching the same shape get sent to the runtime that works.\n\nSource replay job: `{job_id}`\n\n# When to fire\n\nWhen the user's request resembles the source prompt:\n\n> {prompt_summary}\n\nSpecifically, route the prompt to **`{target_runtime}`** instead of `{source_runtime}`.\n\n# Notes for the human\n\nThis skill was auto-drafted by the ATO ops-recipe engine. Review the routing decision and refine the trigger description before relying on it.\n"
    )
}

fn summarize(text: &str, max_chars: usize) -> String {
    let first_line = text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .replace("[user]:", "")
        .trim()
        .to_string();
    if first_line.chars().count() <= max_chars {
        first_line
    } else {
        let t: String = first_line.chars().take(max_chars).collect();
        format!("{}…", t)
    }
}

// ─── Audit table writes ───────────────────────────────────────────────

fn insert_run_row(
    run_id: &str,
    recipe: &OpsRecipe,
    event_seq: i64,
    event_type: &str,
    event_payload: &str,
    started_at: &str,
) {
    let db_path = crate::get_db_path();
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    let _ = conn.execute(
        "INSERT INTO ops_recipe_runs (id, recipe_id, recipe_slug, event_seq, event_type, event_payload, action_type, status, result, error_message, started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', NULL, NULL, ?8, NULL)",
        rusqlite::params![
            run_id,
            recipe.id,
            recipe.slug,
            event_seq,
            event_type,
            event_payload,
            action_name(&recipe.action),
            started_at,
        ],
    );
}

fn finalize_run_row(run_id: &str, outcome: ActionOutcome, finished_at: &str) {
    let db_path = crate::get_db_path();
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    let _ = conn.execute(
        "UPDATE ops_recipe_runs SET status = ?1, result = ?2, error_message = ?3, finished_at = ?4 WHERE id = ?5",
        rusqlite::params![
            outcome.status,
            outcome.result,
            outcome.error,
            finished_at,
            run_id,
        ],
    );
}
