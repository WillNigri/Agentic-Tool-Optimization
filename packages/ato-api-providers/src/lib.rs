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

pub fn registry() -> &'static [ApiProvider] {
    &[
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
        ApiProvider {
            slug: "grok",
            base_url: "https://api.x.ai",
            path: "/v1/chat/completions",
            default_model: "grok-2-latest",
            env_var: "GROK_API_KEY",
            flavor: "openai",
        },
        ApiProvider {
            slug: "deepseek",
            base_url: "https://api.deepseek.com",
            path: "/v1/chat/completions",
            default_model: "deepseek-chat",
            env_var: "DEEPSEEK_API_KEY",
            flavor: "openai",
        },
        ApiProvider {
            slug: "qwen",
            base_url: "https://dashscope-intl.aliyuncs.com",
            path: "/compatible-mode/v1/chat/completions",
            default_model: "qwen-plus",
            env_var: "DASHSCOPE_API_KEY",
            flavor: "openai",
        },
        ApiProvider {
            slug: "openrouter",
            base_url: "https://openrouter.ai",
            path: "/api/v1/chat/completions",
            // OpenRouter is a meta-provider — no default makes
            // sense, so we require --model. Empty string is the
            // sentinel for "must be overridden."
            default_model: "",
            env_var: "OPENROUTER_API_KEY",
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
        assert_eq!(slugs, vec!["minimax", "grok", "deepseek", "qwen", "openrouter"]);
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
