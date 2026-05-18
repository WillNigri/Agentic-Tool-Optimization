// commands/knowledge.rs — agent knowledge ingestion + retrieval
// (the local-RAG pipeline for v2.0 deployed-agent bundles).
//
// PR 4 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `ingest_knowledge_text`    — chunk + embed + persist
//   - `delete_knowledge_chunk`   — single-chunk delete
//   - `delete_knowledge_source`  — bulk delete by (agent_id, source)
//   - `retrieve_knowledge`       — top-K cosine-sim retrieval
//
// Plus all helpers + structs that are knowledge-only:
//   - `KnowledgeChunk` / `RetrievalHit`
//   - `EmbedProvider` (OpenAI / Voyage / Gemini / Cohere / Ollama)
//   - `pick_embed_provider` / `read_provider_active_key`
//   - chunking + cosine-sim + token-approx primitives
//   - `embed_batch` (multi-provider HTTP fan-out)
//
// Out of scope (stays in commands/mod.rs):
//   - `list_agent_knowledge`  — agents domain; will land in PR 28
//     (agents.rs). `retrieve_knowledge` calls it via super::.
//   - `simple_decrypt`         — cross-cutting (also used by secrets);
//     called here via super::.
//
// The `pub use knowledge::*` re-export in commands/mod.rs keeps
// `KnowledgeChunk` accessible as `commands::KnowledgeChunk` for
// `list_agent_knowledge` (still in mod.rs) until PR 28 moves it too.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeChunk {
    pub id: String,
    pub agent_id: String,
    pub source: String,
    pub content: String,
    pub tokens: i64,
    pub position: i64,
    pub embed_model: String,
    pub created_at: String,
    /// Embedding as a flat f32 array. Decoded from the BLOB on read.
    /// Skipped when listing chunks (UI doesn't need 1536 floats per row);
    /// included when generating deploy bundles.
    pub embedding: Option<Vec<f32>>,
}

// v2.0.0 — multi-provider embeddings.
//
// We support five providers, in this preference order. Auto-detected based
// on which API key the user has in `llm_api_keys` (so a user with no
// OpenAI key but a Voyage one gets Voyage automatically — they don't have
// to pick or configure anything).
//
// Each chunk row records `embed_model` so retrieval is always done with
// the same provider that ingested the chunk — vector spaces don't
// interoperate across providers.
//
// Ollama is the offline fallback: needs no key, runs on the user's
// machine, but requires the user to have pulled `nomic-embed-text` first.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbedProvider {
    OpenAI,    // text-embedding-3-small — 1536 dims, $0.02/1M tokens
    Voyage,    // voyage-3 — 1024 dims, ~$0.06/1M tokens
    Gemini,    // text-embedding-004 — 768 dims, free tier available
    Cohere,    // embed-multilingual-light-v3.0 — 384 dims
    Ollama,    // nomic-embed-text — 768 dims, free, local
}

impl EmbedProvider {
    fn provider_id(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Voyage => "voyage",
            Self::Gemini => "gemini",
            Self::Cohere => "cohere",
            Self::Ollama => "ollama",
        }
    }
    fn default_model(&self) -> &'static str {
        match self {
            Self::OpenAI => "text-embedding-3-small",
            Self::Voyage => "voyage-3",
            Self::Gemini => "text-embedding-004",
            Self::Cohere => "embed-multilingual-light-v3.0",
            Self::Ollama => "nomic-embed-text",
        }
    }
    fn dims(&self) -> usize {
        match self {
            Self::OpenAI => 1536,
            Self::Voyage => 1024,
            Self::Gemini => 768,
            Self::Cohere => 384,
            Self::Ollama => 768,
        }
    }
}

/// Pick an embedding provider based on what's available. Auto-detection
/// avoids forcing every user through a picker — the most common case is
/// "I added an OpenAI key" (or a Voyage one) and embeddings should just
/// work. If multiple keys exist we prefer the cheapest first-tier
/// (OpenAI), then Voyage, then Gemini, then Cohere, then Ollama.
fn pick_embed_provider(conn: &rusqlite::Connection) -> Result<(EmbedProvider, Option<String>), String> {
    for p in [EmbedProvider::OpenAI, EmbedProvider::Voyage, EmbedProvider::Gemini, EmbedProvider::Cohere] {
        if let Ok(key) = read_provider_active_key(conn, p.provider_id()) {
            return Ok((p, Some(key)));
        }
    }
    // No cloud key — fall back to local Ollama (no key required). Caller
    // hits localhost:11434, which fails fast if Ollama isn't running.
    Ok((EmbedProvider::Ollama, None))
}

fn read_provider_active_key(
    conn: &rusqlite::Connection,
    provider: &str,
) -> Result<String, String> {
    match conn.query_row::<String, _, _>(
        "SELECT encrypted_key FROM llm_api_keys WHERE provider = ?1 AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
        params![provider],
        |row| row.get(0),
    ) {
        Ok(encrypted) => super::simple_decrypt(&encrypted),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(format!("no {} key", provider)),
        Err(e) => Err(e.to_string()),
    }
}

const EMBED_MODEL: &str = "text-embedding-3-small";
const EMBED_DIMS: usize = 1536;
/// Hard cap so a runaway paste doesn't try to embed an entire book in one
/// request. Beyond this we'd need to batch — keep it simple for v2.0.
const MAX_CHARS_PER_INGEST: usize = 200_000;
/// Target chunk size in characters. ~375 tokens for English text. Small
/// enough that 5–8 chunks fit in any LLM context, large enough that a
/// chunk has actual context.
const CHUNK_CHARS: usize = 1500;
const CHUNK_OVERLAP: usize = 200;

fn chunk_text(text: &str) -> Vec<String> {
    // Naive char-window chunker with overlap. Splits on paragraph boundary
    // when one is within the overlap region so chunks don't tear sentences.
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let end = (i + CHUNK_CHARS).min(chars.len());
        // Try to back off to a paragraph break (\n\n) within the overlap
        // region so we don't tear mid-sentence.
        let mut split_at = end;
        if end < chars.len() {
            let lookback_start = end.saturating_sub(CHUNK_OVERLAP);
            for j in (lookback_start..end).rev() {
                if chars[j] == '\n' && j > 0 && chars[j - 1] == '\n' {
                    split_at = j + 1;
                    break;
                }
            }
        }
        let slice: String = chars[i..split_at].iter().collect();
        let trimmed = slice.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(trimmed);
        }
        if split_at >= chars.len() {
            break;
        }
        i = split_at.saturating_sub(CHUNK_OVERLAP);
    }
    chunks
}

fn f32_vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

// `pub(super)` so `list_agent_knowledge` (still in commands/mod.rs
// under the agents domain) can call this until PR 28 moves it here.
pub(super) fn blob_to_f32_vec(blob: &[u8]) -> Vec<f32> {
    let n = blob.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let bytes = [blob[i * 4], blob[i * 4 + 1], blob[i * 4 + 2], blob[i * 4 + 3]];
        out.push(f32::from_le_bytes(bytes));
    }
    out
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

// (former read_openai_key removed — replaced by pick_embed_provider +
// read_provider_active_key which auto-detect across 5 providers.)

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingRequest<'a> {
    input: &'a [String],
    model: &'a str,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingItem {
    embedding: Vec<f32>,
    index: usize,
}

async fn embed_batch(
    provider: EmbedProvider,
    api_key: Option<&str>,
    inputs: &[String],
) -> Result<Vec<Vec<f32>>, String> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let dims = provider.dims();
    let client = reqwest::Client::new();

    match provider {
        EmbedProvider::OpenAI => {
            let key = api_key.ok_or("OpenAI embedder requires an API key")?;
            let payload = OpenAIEmbeddingRequest { input: inputs, model: provider.default_model() };
            let r = client
                .post("https://api.openai.com/v1/embeddings")
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("OpenAI embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("OpenAI embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let parsed: OpenAIEmbeddingResponse = r.json().await.map_err(|e| e.to_string())?;
            let mut out: Vec<Vec<f32>> = vec![Vec::new(); inputs.len()];
            for item in parsed.data {
                if item.index < out.len() && item.embedding.len() == dims {
                    out[item.index] = item.embedding;
                }
            }
            if out.iter().any(|v| v.len() != dims) {
                return Err("OpenAI embeddings: missing/wrong-dim vector".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Voyage => {
            // Voyage's API is OpenAI-compatible-ish but NOT identical — uses
            // `input` array, model name `voyage-3`. Returns same `data[].embedding`.
            let key = api_key.ok_or("Voyage embedder requires an API key")?;
            let payload = serde_json::json!({
                "input": inputs,
                "model": provider.default_model(),
            });
            let r = client
                .post("https://api.voyageai.com/v1/embeddings")
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("Voyage embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("Voyage embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            let arr = body.get("data").and_then(|d| d.as_array())
                .ok_or("Voyage: missing `data` array")?;
            let mut out: Vec<Vec<f32>> = vec![Vec::new(); inputs.len()];
            for item in arr {
                let idx = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let emb = item.get("embedding").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                if idx < out.len() && emb.len() == dims {
                    out[idx] = emb;
                }
            }
            if out.iter().any(|v| v.len() != dims) {
                return Err("Voyage embeddings: missing/wrong-dim vector".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Gemini => {
            // Gemini exposes batch embeddings via `:batchEmbedContents`. Single
            // request, parallel embedding requests in the body.
            let key = api_key.ok_or("Gemini embedder requires an API key")?;
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents?key={}",
                provider.default_model(),
                key,
            );
            let requests: Vec<serde_json::Value> = inputs.iter().map(|t| serde_json::json!({
                "model": format!("models/{}", provider.default_model()),
                "content": { "parts": [{ "text": t }] },
            })).collect();
            let payload = serde_json::json!({ "requests": requests });
            let r = client.post(&url).json(&payload).send().await
                .map_err(|e| format!("Gemini embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("Gemini embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            let arr = body.get("embeddings").and_then(|d| d.as_array())
                .ok_or("Gemini: missing `embeddings` array")?;
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for item in arr {
                let emb = item.get("values").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                out.push(emb);
            }
            if out.len() != inputs.len() || out.iter().any(|v| v.len() != dims) {
                return Err("Gemini embeddings: count or dim mismatch".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Cohere => {
            let key = api_key.ok_or("Cohere embedder requires an API key")?;
            let payload = serde_json::json!({
                "texts": inputs,
                "model": provider.default_model(),
                "input_type": "search_document",
            });
            let r = client
                .post("https://api.cohere.com/v2/embed")
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("Cohere embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("Cohere embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            // Cohere returns {embeddings: {float: [[...]]}} or {embeddings: [[...]]}
            let arr = body.pointer("/embeddings/float")
                .or_else(|| body.get("embeddings"))
                .and_then(|d| d.as_array())
                .ok_or("Cohere: missing embeddings array")?;
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for item in arr {
                let emb = item.as_array()
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                out.push(emb);
            }
            if out.len() != inputs.len() || out.iter().any(|v| v.len() != dims) {
                return Err("Cohere embeddings: count or dim mismatch".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Ollama => {
            // Local fallback. Hits localhost:11434/api/embed. User must have
            // run `ollama pull nomic-embed-text` once, otherwise this errors.
            // The deployed bundle CAN'T use Ollama (it's local) — only ingest
            // works with this provider for now.
            let model = provider.default_model();
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for input in inputs {
                let r = client
                    .post("http://localhost:11434/api/embeddings")
                    .json(&serde_json::json!({ "model": model, "prompt": input }))
                    .send()
                    .await
                    .map_err(|e| format!("Ollama not reachable on localhost:11434 — start it with `ollama serve` and pull the model with `ollama pull {}`. Underlying: {}", model, e))?;
                if !r.status().is_success() {
                    return Err(format!("Ollama embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
                }
                let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
                let emb = body.get("embedding").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                if emb.len() != dims {
                    return Err(format!("Ollama embeddings: model returned {} dims, expected {}", emb.len(), dims));
                }
                out.push(emb);
            }
            Ok(out)
        }
    }
}

/// Approximate token count — char count / 4 for English text. Matches
/// OpenAI's rough rule of thumb close enough for the UI's storage display.
fn approx_tokens(s: &str) -> i64 {
    (s.chars().count() / 4) as i64
}

/// Ingest a chunk of plain text (typically a .md or .txt file's contents).
/// Replaces any prior chunks for the same `source` so re-uploading the same
/// file overwrites instead of duplicating.
#[tauri::command]
pub async fn ingest_knowledge_text(
    db: State<'_, DbState>,
    agent_id: String,
    source: String,
    content: String,
) -> Result<Vec<KnowledgeChunk>, String> {
    if content.is_empty() {
        return Err("content cannot be empty".to_string());
    }
    if content.len() > MAX_CHARS_PER_INGEST {
        return Err(format!(
            "content too large ({} chars, max {}). Split the file before uploading.",
            content.len(),
            MAX_CHARS_PER_INGEST
        ));
    }

    let chunks = chunk_text(&content);
    if chunks.is_empty() {
        return Err("nothing to embed — the file is whitespace only".to_string());
    }

    // Pick provider + key BEFORE the network call so we fail fast on
    // a misconfigured machine. Auto-detects based on which provider key
    // the user has on file. Ollama is used as the offline fallback.
    let (provider, api_key) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        pick_embed_provider(&conn)?
    };

    let embeddings = embed_batch(provider, api_key.as_deref(), &chunks).await?;
    if embeddings.len() != chunks.len() {
        return Err("embedder returned the wrong number of vectors".to_string());
    }

    let model_id = provider.default_model();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // Replace prior chunks from this same source so re-uploading overwrites.
    conn.execute(
        "DELETE FROM agent_knowledge_chunks WHERE agent_id = ?1 AND source = ?2",
        params![agent_id, source],
    )
    .map_err(|e| e.to_string())?;

    let mut out: Vec<KnowledgeChunk> = Vec::with_capacity(chunks.len());
    for (i, (text, embedding)) in chunks.into_iter().zip(embeddings.into_iter()).enumerate() {
        let id = uuid::Uuid::new_v4().to_string();
        let tokens = approx_tokens(&text);
        let blob = f32_vec_to_blob(&embedding);
        conn.execute(
            "INSERT INTO agent_knowledge_chunks
             (id, agent_id, source, content, tokens, position, embedding, embed_model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, agent_id, source, text, tokens, i as i64, blob, model_id, now],
        )
        .map_err(|e| e.to_string())?;
        out.push(KnowledgeChunk {
            id,
            agent_id: agent_id.clone(),
            source: source.clone(),
            content: text,
            tokens,
            position: i as i64,
            embed_model: model_id.to_string(),
            created_at: now.clone(),
            embedding: Some(embedding),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn delete_knowledge_chunk(
    db: State<'_, DbState>,
    chunk_id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM agent_knowledge_chunks WHERE id = ?1",
        params![chunk_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_knowledge_source(
    db: State<'_, DbState>,
    agent_id: String,
    source: String,
) -> Result<u64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let n = conn
        .execute(
            "DELETE FROM agent_knowledge_chunks WHERE agent_id = ?1 AND source = ?2",
            params![agent_id, source],
        )
        .map_err(|e| e.to_string())?;
    Ok(n as u64)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalHit {
    pub chunk: KnowledgeChunk,
    pub score: f32,
}

/// Retrieval — embed the query and return the top-K matching chunks. Used
/// by the "Test retrieval" panel in the UI; deploy bundles do their own
/// cosine-sim at request time using the inlined chunks.
#[tauri::command]
pub async fn retrieve_knowledge(
    db: State<'_, DbState>,
    agent_id: String,
    query: String,
    k: Option<u32>,
) -> Result<Vec<RetrievalHit>, String> {
    let k = k.unwrap_or(5).max(1).min(20) as usize;
    if query.trim().is_empty() {
        return Err("query cannot be empty".to_string());
    }

    // Pick provider — must match whichever was used to ingest the chunks
    // we're retrieving against. For v2 alpha we route via the same
    // auto-detect; if the user changed key sets between ingest and
    // retrieve, the cosine scores won't be meaningful (different vector
    // spaces). The chunk's stored `embed_model` is the source of truth
    // for "which provider should retrieve use" — wiring that lookup is a
    // v2.0.x follow-up.
    let (provider, api_key) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        pick_embed_provider(&conn)?
    };

    let query_embeddings = embed_batch(provider, api_key.as_deref(), &[query.clone()]).await?;
    let query_vec = query_embeddings
        .into_iter()
        .next()
        .ok_or_else(|| "embedder returned no vector for the query".to_string())?;

    let chunks = super::list_agent_knowledge(db, agent_id, Some(true))?;
    let mut scored: Vec<RetrievalHit> = chunks
        .into_iter()
        .filter_map(|c| {
            let v = c.embedding.clone().unwrap_or_default();
            if v.is_empty() {
                None
            } else {
                let s = cosine_similarity(&query_vec, &v);
                Some(RetrievalHit { chunk: c, score: s })
            }
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    Ok(scored)
}

