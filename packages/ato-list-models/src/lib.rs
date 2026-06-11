// ato-list-models — live model discovery for ATO's 8 API providers.
//
// Why this exists (v2.15.0 Slice C, war_room 0D398F74):
//   Pre-2.15.0 every provider had a hardcoded `default_model` in the
//   registry that went stale (we shipped 2.14.5 just to bump gemini-
//   2.5-flash → gemini-3-flash-preview because Google was 503-ing).
//   Codex's verdict: "we should not be hardcoding which model to use,
//   we should allow the user even to choose which model within ATO."
//
// Design (war_room 0D398F74 + codex's amendment):
//   - DO put this in its own crate (NOT in ato-api-providers, which is
//     pure type-defs + registry, no HTTP).
//   - DO include `source: 'live' | 'curated_fallback'` so consumers see
//     model provenance honestly (avoids the "looks live but isn't" trap).
//   - DO add a small in-memory TTL cache keyed by (provider, key_fp).
//     React Query keeps a 1h UI cache; this is the backend's 10min
//     cache so multiple call sites (PromptBar, Settings, agent picker)
//     don't each round-trip.
//   - DO NOT persist to SQLite in this slice (codex: "Do not add
//     SQLite cache/invalidation in this slice").

use ato_api_providers::ApiProvider;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

mod providers;

/// Normalized model info returned across all providers. The `source`
/// field is the load-bearing honesty bit codex insisted on.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    /// The string that goes into the dispatch path (e.g. "gemini-3-flash-preview",
    /// "claude-sonnet-4-6", "gpt-4o"). This is what the user actually picks
    /// in the model picker UI.
    pub id: String,
    /// Human-friendly label for the dropdown. Usually the same as `id`
    /// for providers that don't return a separate display name.
    pub display_name: String,
    /// Provider's "owned_by" or "developer" field if available. Some
    /// providers (OpenRouter) return third-party models with the original
    /// owner attribution.
    pub owned_by: Option<String>,
}

/// Honest provenance label on a model list response — codex's
/// load-bearing field. Consumers (PromptBar, Settings → Models) MUST
/// surface this to the user; "looks live but isn't" is the failure
/// mode this exists to prevent.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelListSource {
    /// The provider's API returned this list when we queried just now
    /// (or within the cache TTL window).
    Live,
    /// The provider has no public list-models endpoint we know of,
    /// AND/OR the live fetch failed. We're falling back to a list
    /// curated in this crate. Consumers SHOULD show "curated" or
    /// similar so the user knows it isn't guaranteed current.
    CuratedFallback,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelListResponse {
    pub provider_slug: String,
    pub source: ModelListSource,
    pub models: Vec<ModelInfo>,
    /// When the list was fetched (or pulled from cache). RFC3339.
    pub fetched_at: String,
    /// Only set when source == CuratedFallback AND a live fetch was
    /// attempted. The user-facing message explaining the fallback.
    pub fallback_reason: Option<String>,
}

#[derive(Debug)]
pub enum ListModelsError {
    UnsupportedProvider(String),
    Network(String),
    Auth(u16, String),
    Parse(String),
}

impl std::fmt::Display for ListModelsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListModelsError::UnsupportedProvider(s) => write!(f, "unsupported provider: {}", s),
            ListModelsError::Network(s) => write!(f, "network error: {}", s),
            ListModelsError::Auth(code, body) => write!(f, "auth/HTTP {}: {}", code, body),
            ListModelsError::Parse(s) => write!(f, "parse error: {}", s),
        }
    }
}

impl std::error::Error for ListModelsError {}

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
struct CacheEntry {
    response: ModelListResponse,
    cached_at: Instant,
}

/// In-process cache keyed by (provider_slug, key_fingerprint).
/// 10 min TTL. React Query's 1h cache sits in front of this.
fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key_fingerprint(api_key: &str) -> String {
    let h = Sha256::digest(api_key.as_bytes());
    h[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn cache_key(provider_slug: &str, api_key: &str) -> String {
    format!("{}:{}", provider_slug, key_fingerprint(api_key))
}

/// Public entry: fetch (or return cached) model list for a provider.
/// Caller supplies the plaintext API key. The cache lookup is
/// fingerprinted so different keys for the same provider don't share
/// a cache entry (a teammate's key with different model access wouldn't
/// poison your cache).
pub async fn list_models(
    provider: &ApiProvider,
    api_key: &str,
) -> Result<ModelListResponse, ListModelsError> {
    let ckey = cache_key(provider.slug, api_key);
    {
        let map = cache().lock().await;
        if let Some(entry) = map.get(&ckey) {
            if entry.cached_at.elapsed() < CACHE_TTL {
                return Ok(entry.response.clone());
            }
        }
    }
    let response = providers::fetch_for_provider(provider, api_key).await?;
    let mut map = cache().lock().await;
    map.insert(
        ckey,
        CacheEntry {
            response: response.clone(),
            cached_at: Instant::now(),
        },
    );
    Ok(response)
}

/// Drop everything from the cache. Used by tests + by the "Pull live"
/// button in Settings → Models so a user-triggered refresh actually
/// goes out to the network even within the TTL window.
pub async fn invalidate_cache() {
    let mut map = cache().lock().await;
    map.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_fingerprint_is_deterministic_and_short() {
        let k = "sk-test-12345";
        let fp1 = key_fingerprint(k);
        let fp2 = key_fingerprint(k);
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), 16);
        // Different keys produce different fingerprints.
        assert_ne!(key_fingerprint("sk-test-12345"), key_fingerprint("sk-test-99999"));
    }

    #[test]
    fn cache_key_includes_provider_and_fingerprint() {
        let k1 = cache_key("google", "AQ.test");
        let k2 = cache_key("openai", "AQ.test");
        assert_ne!(k1, k2, "different providers must not collide");
        let k3 = cache_key("google", "AQ.other");
        assert_ne!(k1, k3, "different keys must not share cache slot");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalidate_cache_clears_everything() {
        // Seed the cache manually.
        {
            let mut map = cache().lock().await;
            map.insert(
                "google:abc".to_string(),
                CacheEntry {
                    response: ModelListResponse {
                        provider_slug: "google".to_string(),
                        source: ModelListSource::Live,
                        models: vec![],
                        fetched_at: "2026-06-11T00:00:00Z".to_string(),
                        fallback_reason: None,
                    },
                    cached_at: Instant::now(),
                },
            );
            assert_eq!(map.len(), 1);
        }
        invalidate_cache().await;
        let map = cache().lock().await;
        assert_eq!(map.len(), 0);
    }
}
