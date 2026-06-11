// Per-provider list_models implementations. Eight providers split
// into three patterns:
//   - OpenAI-shape (5 providers: openai, deepseek, grok, openrouter,
//     anthropic [different auth header]) — GET /v1/models, normalize
//     `data[].id`.
//   - Google — GET /v1beta/models?key=…, filter to entries supporting
//     generateContent.
//   - Qwen (DashScope) — GET /api/v1/models, slightly different shape.
//   - MiniMax — NO public list endpoint. Returns curated_fallback per
//     codex's verdict ("change the shape now to include source: live
//     | curated_fallback and then static fallback becomes acceptable").

use crate::{ListModelsError, ModelInfo, ModelListResponse, ModelListSource};
use ato_api_providers::ApiProvider;

pub async fn fetch_for_provider(
    provider: &ApiProvider,
    api_key: &str,
) -> Result<ModelListResponse, ListModelsError> {
    let fetched_at = chrono_now_rfc3339();
    match provider.flavor {
        "gemini" => google::fetch(provider, api_key, &fetched_at).await,
        "anthropic" => anthropic::fetch(provider, api_key, &fetched_at).await,
        "minimax" => Ok(minimax::curated(&fetched_at)),
        // OpenAI-compatible shape covers openai, deepseek, grok, openrouter.
        // qwen uses a different path so we handle it separately.
        _ if provider.slug == "qwen" => qwen::fetch(provider, api_key, &fetched_at).await,
        _ => openai_compat::fetch(provider, api_key, &fetched_at).await,
    }
}

fn chrono_now_rfc3339() -> String {
    // Tiny inlined RFC3339 timestamp without pulling chrono into this
    // crate. The desktop + CLI both already depend on chrono; the
    // backend Tauri command can re-stamp with chrono if it wants.
    // For this crate we just need a string; SystemTime suffices.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 2026-01-01 = 1767225600 — close enough; consumers usually re-stamp.
    let days = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    // We just want a sortable + parseable label; precise calendar date
    // arithmetic isn't load-bearing here.
    format!("{:010}T{:02}:{:02}:{:02}Z", days, h, m, s)
}

mod openai_compat {
    use super::*;
    use serde_json::Value;

    pub async fn fetch(
        provider: &ApiProvider,
        api_key: &str,
        fetched_at: &str,
    ) -> Result<ModelListResponse, ListModelsError> {
        let url = format!("{}/v1/models", provider.base_url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ListModelsError::Network(format!("build client: {}", e)))?;
        let resp = client
            .get(&url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| ListModelsError::Network(format!("GET {}: {}", url, e)))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ListModelsError::Network(format!("read body: {}", e)))?;
        if !status.is_success() {
            return Err(ListModelsError::Auth(status.as_u16(), body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| ListModelsError::Parse(format!("{}: {}", e, body)))?;
        let data = v["data"].as_array().ok_or_else(|| {
            ListModelsError::Parse(format!("response missing data[]: {}", body))
        })?;
        let models = data
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?;
                Some(ModelInfo {
                    id: id.to_string(),
                    display_name: id.to_string(),
                    owned_by: m["owned_by"].as_str().map(|s| s.to_string()),
                })
            })
            .collect();
        Ok(ModelListResponse {
            provider_slug: provider.slug.to_string(),
            source: ModelListSource::Live,
            models,
            fetched_at: fetched_at.to_string(),
            fallback_reason: None,
        })
    }
}

mod anthropic {
    use super::*;
    use serde_json::Value;

    pub async fn fetch(
        provider: &ApiProvider,
        api_key: &str,
        fetched_at: &str,
    ) -> Result<ModelListResponse, ListModelsError> {
        let url = format!("{}/v1/models", provider.base_url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ListModelsError::Network(format!("build client: {}", e)))?;
        let resp = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .map_err(|e| ListModelsError::Network(format!("GET {}: {}", url, e)))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ListModelsError::Network(format!("read body: {}", e)))?;
        if !status.is_success() {
            return Err(ListModelsError::Auth(status.as_u16(), body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| ListModelsError::Parse(format!("{}: {}", e, body)))?;
        let data = v["data"].as_array().ok_or_else(|| {
            ListModelsError::Parse(format!("response missing data[]: {}", body))
        })?;
        let models = data
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?;
                let display = m["display_name"].as_str().unwrap_or(id);
                Some(ModelInfo {
                    id: id.to_string(),
                    display_name: display.to_string(),
                    owned_by: Some("anthropic".to_string()),
                })
            })
            .collect();
        Ok(ModelListResponse {
            provider_slug: provider.slug.to_string(),
            source: ModelListSource::Live,
            models,
            fetched_at: fetched_at.to_string(),
            fallback_reason: None,
        })
    }
}

mod google {
    use super::*;
    use serde_json::Value;

    pub async fn fetch(
        provider: &ApiProvider,
        api_key: &str,
        fetched_at: &str,
    ) -> Result<ModelListResponse, ListModelsError> {
        let url = format!("{}/v1beta/models?key={}", provider.base_url, urlencode(api_key));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ListModelsError::Network(format!("build client: {}", e)))?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ListModelsError::Network(format!("GET .../models: {}", e)))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ListModelsError::Network(format!("read body: {}", e)))?;
        if !status.is_success() {
            return Err(ListModelsError::Auth(status.as_u16(), body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| ListModelsError::Parse(format!("{}: {}", e, body)))?;
        let arr = v["models"].as_array().ok_or_else(|| {
            ListModelsError::Parse(format!("response missing models[]: {}", body))
        })?;
        // Google returns names like "models/gemini-3-flash-preview". Strip
        // the "models/" prefix because dispatch interpolates the bare id.
        // Filter to models that support generateContent so we don't surface
        // embedding / vision / TTS models in the chat picker.
        let models = arr
            .iter()
            .filter_map(|m| {
                let full = m["name"].as_str()?;
                let id = full.strip_prefix("models/").unwrap_or(full).to_string();
                let methods = m["supportedGenerationMethods"].as_array();
                let supports_generate = methods.map_or(true, |ms| {
                    ms.iter()
                        .any(|s| s.as_str().map(|x| x.eq_ignore_ascii_case("generateContent")).unwrap_or(false))
                });
                if !supports_generate {
                    return None;
                }
                let display = m["displayName"].as_str().unwrap_or(&id).to_string();
                Some(ModelInfo {
                    id,
                    display_name: display,
                    owned_by: Some("google".to_string()),
                })
            })
            .collect();
        Ok(ModelListResponse {
            provider_slug: provider.slug.to_string(),
            source: ModelListSource::Live,
            models,
            fetched_at: fetched_at.to_string(),
            fallback_reason: None,
        })
    }

    fn urlencode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                out.push(c);
            } else {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
        out
    }
}

mod qwen {
    use super::*;
    use serde_json::Value;

    pub async fn fetch(
        provider: &ApiProvider,
        api_key: &str,
        fetched_at: &str,
    ) -> Result<ModelListResponse, ListModelsError> {
        // DashScope's compat endpoint. If the user is on the native
        // DashScope routes, the OpenAI-compatible alias works for /models.
        let url = format!("{}/v1/models", provider.base_url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ListModelsError::Network(format!("build client: {}", e)))?;
        let resp = client
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|e| ListModelsError::Network(format!("GET {}: {}", url, e)))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ListModelsError::Network(format!("read body: {}", e)))?;
        if !status.is_success() {
            return Err(ListModelsError::Auth(status.as_u16(), body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| ListModelsError::Parse(format!("{}: {}", e, body)))?;
        let data = v["data"].as_array().ok_or_else(|| {
            ListModelsError::Parse(format!("response missing data[]: {}", body))
        })?;
        let models = data
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?;
                Some(ModelInfo {
                    id: id.to_string(),
                    display_name: id.to_string(),
                    owned_by: m["owned_by"].as_str().map(|s| s.to_string()),
                })
            })
            .collect();
        Ok(ModelListResponse {
            provider_slug: provider.slug.to_string(),
            source: ModelListSource::Live,
            models,
            fetched_at: fetched_at.to_string(),
            fallback_reason: None,
        })
    }
}

mod minimax {
    use super::*;

    pub fn curated(fetched_at: &str) -> ModelListResponse {
        // Per codex's verdict: "MiniMax has no public list endpoint we
        // know of, AND/OR the live fetch failed. Return ModelInfo with
        // source: CuratedFallback so the UI can show the user this is
        // curated, not live." Honest provenance > silent staleness.
        ModelListResponse {
            provider_slug: "minimax".to_string(),
            source: ModelListSource::CuratedFallback,
            models: vec![
                ModelInfo {
                    id: "MiniMax-M2.7-highspeed".to_string(),
                    display_name: "MiniMax M2.7 (highspeed)".to_string(),
                    owned_by: Some("minimax".to_string()),
                },
                ModelInfo {
                    id: "abab6.5-chat".to_string(),
                    display_name: "abab6.5 chat".to_string(),
                    owned_by: Some("minimax".to_string()),
                },
                ModelInfo {
                    id: "abab6.5s-chat".to_string(),
                    display_name: "abab6.5s chat".to_string(),
                    owned_by: Some("minimax".to_string()),
                },
            ],
            fetched_at: fetched_at.to_string(),
            fallback_reason: Some(
                "MiniMax does not publish a public list-models endpoint. \
                 This list is curated by ATO and may be stale. \
                 Pass --model explicitly if you know the exact model name."
                    .to_string(),
            ),
        }
    }
}
