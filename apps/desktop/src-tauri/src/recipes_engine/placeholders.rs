// v2.7.14 — extracted from recipes_engine.rs (v2.8.0 split).
// Owns: template-string substitution from event fields. Used by
// every action executor (notify_human's text, post_webhook's URL +
// body, run_script's args, etc.) via pub(super) re-export from
// the parent module.

use crate::events::AtoEvent;

const KNOWN_PLACEHOLDERS: &[&str] = &[
    "{{source_runtime}}",
    "{{target_runtime}}",
    "{{agent_slug}}",
    "{{previous_runtime}}",
];

#[derive(Default)]
pub(super) struct EventFields {
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

pub(super) fn extract_event_fields(event: &AtoEvent) -> EventFields {
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
pub(super) fn first_missing_placeholder(
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
pub(super) fn redact_url(url: &str) -> String {
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
pub(super) fn apply_substitution(template: &str, fields: &EventFields, json_safe: bool) -> String {
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

pub(super) fn substitute_simple_placeholders(template: &str, event: &AtoEvent) -> String {
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
