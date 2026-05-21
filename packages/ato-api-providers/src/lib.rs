// ato-api-providers — shared registry of API-key dispatch targets.
//
// v2.3.21 introduced API-key dispatch in the CLI; v2.3.26 added the
// desktop's async mirror. Each carried its own copy of the 5-entry
// provider registry. Both commits acknowledged the drift risk and
// noted a follow-up to extract here. This crate is that follow-up.
//
// Scope: ApiProvider struct + the registry() list. Dispatch HTTP
// logic stays in the consuming crate because one uses blocking
// reqwest and the other uses async — duplicating ~80 lines of
// dispatch code is acceptable; duplicating the URL/model/env-var
// list was the actual drift risk.

#[derive(Debug, Clone, Copy)]
pub struct ApiProvider {
    pub slug: &'static str,
    pub base_url: &'static str,
    pub path: &'static str,
    pub default_model: &'static str,
    pub env_var: &'static str,
    /// "openai" = standard chat-completions shape with HTTP-status
    /// success check. "minimax" = 200 OK + base_resp.status_code
    /// (0=success).
    pub flavor: &'static str,
}

// Each entry below carries a `// last verified:` comment with the
// most recent date the smoke test (`ato runtimes test-providers`)
// actually round-tripped a dispatch against this entry. Bump it on
// every verification. Entries without a key configured on the dev
// machine carry `UNVERIFIED` — they're registered based on the
// vendor's published API shape but haven't been roundtripped end-
// to-end. Treat UNVERIFIED entries as "best-effort"; the smoke test
// surfaces drift before users hit it.
pub fn registry() -> &'static [ApiProvider] {
    &[
        // last verified: 2026-05-13 ✓ (dispatch returned 2-char reply)
        ApiProvider {
            slug: "minimax",
            base_url: "https://api.minimax.io",
            path: "/v1/text/chatcompletion_v2",
            // MiniMax-M2.7-highspeed is the Plus Token Plan default;
            // users on other tiers override with --model. The older
            // MiniMax-M2 and MiniMax-Text-01 are gated on the
            // metered API, not the subscription.
            default_model: "MiniMax-M2.7-highspeed",
            env_var: "MINIMAX_API_KEY",
            flavor: "minimax",
        },
        // last verified: UNVERIFIED (no GROK_API_KEY on dev machine
        // as of 2026-05-13). xAI keeps `-latest` aliases pointed at
        // their current production model so this default is unlikely
        // to drift, but the smoke test should be run on a machine
        // with a real key before claiming this works.
        ApiProvider {
            slug: "grok",
            base_url: "https://api.x.ai",
            path: "/v1/chat/completions",
            default_model: "grok-2-latest",
            env_var: "GROK_API_KEY",
            flavor: "openai",
        },
        // last verified: UNVERIFIED (no DEEPSEEK_API_KEY on dev
        // machine as of 2026-05-13). `deepseek-chat` is DeepSeek's
        // long-standing alias for their current production chat
        // model.
        ApiProvider {
            slug: "deepseek",
            base_url: "https://api.deepseek.com",
            path: "/v1/chat/completions",
            default_model: "deepseek-chat",
            env_var: "DEEPSEEK_API_KEY",
            flavor: "openai",
        },
        // last verified: UNVERIFIED (no DASHSCOPE_API_KEY on dev
        // machine as of 2026-05-13). Going through Alibaba's
        // dashscope-intl endpoint; the OpenAI-compatible path under
        // /compatible-mode/ is the documented entry point for
        // non-China users.
        ApiProvider {
            slug: "qwen",
            base_url: "https://dashscope-intl.aliyuncs.com",
            path: "/compatible-mode/v1/chat/completions",
            default_model: "qwen-plus",
            env_var: "DASHSCOPE_API_KEY",
            flavor: "openai",
        },
        // last verified: UNVERIFIED (no OPENROUTER_API_KEY on dev
        // machine as of 2026-05-13). Meta-provider — no default
        // model since the user has to pick from OpenRouter's full
        // catalogue.
        ApiProvider {
            slug: "openrouter",
            base_url: "https://openrouter.ai",
            path: "/api/v1/chat/completions",
            default_model: "",
            env_var: "OPENROUTER_API_KEY",
            flavor: "openai",
        },
        // last verified: 2026-05-13 ✓ (dispatch returned 2-char reply
        //                                with model gemini-2.5-flash)
        // v2.4.2 — Google Gemini API. Slug intentionally `google`
        // (vendor name) rather than `gemini` to avoid colliding with
        // the existing `gemini` CLI runtime; users with the Gemini
        // CLI installed keep that path, users with only an API key
        // dispatch via `ato dispatch google "..."`.
        //
        // Gemini's REST API is structurally different from OpenAI's:
        // - Auth is `?key=<API_KEY>` query parameter, not Bearer.
        // - Request body uses `contents[]` not `messages[]`, with
        //   role values `user` / `model` (not `user` / `assistant`).
        // - URL embeds the model name as a path segment, not a
        //   field in the body.
        // The "gemini" flavor in api_dispatch handles all three.
        // last verified: UNVERIFIED (no ANTHROPIC_API_KEY on dev
        // machine as of 2026-05-14). Slug intentionally `anthropic`
        // (vendor) rather than `claude` to avoid colliding with the
        // existing `claude` CLI runtime — users with the Claude CLI
        // installed keep that subprocess path, users with only an
        // API key (or who want to skip the CLI entirely for BYOK
        // billing transparency) dispatch via `ato dispatch
        // anthropic "..."`. Same naming pattern as `google` /
        // `gemini`.
        //
        // Anthropic's Messages API is structurally different from
        // OpenAI's chat-completions:
        // - Auth via `x-api-key` header, not `Authorization: Bearer`.
        // - Required `anthropic-version: 2023-06-01` header.
        // - Request body uses `messages[]` (compatible with OpenAI
        //   shape for role/content) but mandates `max_tokens` and
        //   `system` as top-level fields.
        // - Response: `content[]` array of typed blocks (only
        //   `type: text` consumed today) instead of
        //   `choices[].message.content`.
        // - Usage: `input_tokens` / `output_tokens` (not
        //   `prompt_tokens` / `completion_tokens`).
        // The "anthropic" flavor in api_dispatch handles all of these.
        ApiProvider {
            slug: "anthropic",
            base_url: "https://api.anthropic.com",
            path: "/v1/messages",
            // claude-sonnet-4-6 = good cost/perf default; users on
            // Opus subscriptions override with --model.
            default_model: "claude-sonnet-4-6",
            env_var: "ANTHROPIC_API_KEY",
            flavor: "anthropic",
        },
        ApiProvider {
            slug: "google",
            base_url: "https://generativelanguage.googleapis.com",
            // Model is interpolated into the path at dispatch time
            // because Gemini's URL shape requires it; the literal
            // `{model}` token is the placeholder the dispatcher
            // substitutes.
            path: "/v1beta/models/{model}:generateContent",
            // gemini-2.0-flash was deprecated for new users 2026-Q1.
            // gemini-2.5-flash is the current low-latency default;
            // users on Pro plans override with --model gemini-2.5-pro.
            default_model: "gemini-2.5-flash",
            env_var: "GEMINI_API_KEY",
            flavor: "gemini",
        },
        // v2.7.14 — OpenAI API provider. Closes the v2.8.x docket
        // item from `project_v2_7_8_agent_perms_shipped.md`:
        // "Codex has no OpenAI API provider in the registry.
        //  claude→anthropic and gemini→google auto-fallback work;
        //  codex→openai queued for v2.8.x."
        //
        // Slug intentionally `openai` (vendor) rather than `codex` to
        // avoid colliding with the existing `codex` CLI runtime —
        // users with the Codex CLI installed keep that subprocess
        // path, users with only an API key dispatch via `ato
        // dispatch openai "..."` (or are auto-routed from `codex`
        // by the byok.rs mapping at apps/cli/src/byok.rs:34-46 when
        // the CLI binary isn't on PATH).
        //
        // Standard chat-completions shape — same flavor as
        // grok / deepseek / qwen / openrouter (they're all OpenAI-
        // compatible). default_model is gpt-4o-mini (low-latency,
        // metered) — users on a Plus/Pro tier with subscription
        // access to gpt-4o or o1 override with --model.
        ApiProvider {
            slug: "openai",
            base_url: "https://api.openai.com",
            path: "/v1/chat/completions",
            default_model: "gpt-4o-mini",
            env_var: "OPENAI_API_KEY",
            flavor: "openai",
        },
    ]
}

pub fn find_provider(slug: &str) -> Option<&'static ApiProvider> {
    let lower = slug.to_ascii_lowercase();
    registry().iter().find(|p| p.slug == lower.as_str())
}

pub fn is_api_provider(slug: &str) -> bool {
    find_provider(slug).is_some()
}

/// List of (slug, label) pairs the desktop's runtime picker uses to
/// know which API providers exist. Labels are used in dropdowns.
/// Pulled from registry() so adding a new provider only touches
/// one place.
pub fn slugs_and_labels() -> Vec<(&'static str, &'static str)> {
    registry()
        .iter()
        .map(|p| (p.slug, label_for(p.slug).unwrap_or(p.slug)))
        .collect()
}

/// Display label for a provider slug. Returns None for unknown slugs
/// — callers decide the fallback (often the slug itself, lifetimed
/// however they want). Caller-side fallback avoids the
/// "can't promote non-static slug to &'static" error.
pub fn label_for(slug: &str) -> Option<&'static str> {
    match slug {
        "minimax" => Some("MiniMax"),
        "grok" => Some("Grok"),
        "deepseek" => Some("DeepSeek"),
        "qwen" => Some("Qwen"),
        "openrouter" => Some("OpenRouter"),
        "google" => Some("Google (Gemini)"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_providers() {
        assert!(find_provider("minimax").is_some());
        assert!(find_provider("MiniMax").is_some());
        assert!(find_provider("nonexistent").is_none());
    }

    #[test]
    fn registry_has_expected_providers() {
        let slugs: Vec<&str> = registry().iter().map(|p| p.slug).collect();
        // 2026-05-19 — added "anthropic" between openrouter + google when
        // BYOK Anthropic API key dispatch landed.
        // v2.7.14 — added "openai" after "google" to close the v2.8.x
        // docket item: codex CLI now has an API-key fallback path via
        // OPENAI_API_KEY (matches the claude→anthropic / gemini→google
        // pattern). Test was checking an exact-list invariant; update
        // to keep ratcheting on additions.
        assert_eq!(
            slugs,
            vec![
                "minimax", "grok", "deepseek", "qwen", "openrouter",
                "anthropic", "google", "openai",
            ]
        );
    }

    #[test]
    fn openai_uses_chat_completions_path_and_openai_flavor() {
        let p = find_provider("openai").expect("openai provider registered");
        assert_eq!(p.path, "/v1/chat/completions");
        assert_eq!(p.flavor, "openai");
        assert_eq!(p.env_var, "OPENAI_API_KEY");
        assert!(!p.default_model.is_empty());
        assert!(p.default_model.starts_with("gpt-"));
    }

    #[test]
    fn google_uses_gemini_flavor_and_model_path_template() {
        let p = find_provider("google").unwrap();
        assert_eq!(p.flavor, "gemini");
        assert!(p.path.contains("{model}"));
        // The default model is verified live by `ato runtimes
        // test-providers`; this test only asserts it's non-empty so
        // a future model bump (when Google deprecates the current
        // default again) doesn't have to also touch this test.
        assert!(!p.default_model.is_empty());
        assert!(p.default_model.starts_with("gemini-"));
    }

    #[test]
    fn openrouter_has_no_default_model() {
        let p = find_provider("openrouter").unwrap();
        assert_eq!(p.default_model, "");
    }

    #[test]
    fn minimax_uses_minimax_flavor() {
        let p = find_provider("minimax").unwrap();
        assert_eq!(p.flavor, "minimax");
    }
}
