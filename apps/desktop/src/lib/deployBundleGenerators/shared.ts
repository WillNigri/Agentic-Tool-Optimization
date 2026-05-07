// v2.0.0 Wave 3 — pieces shared across deploy bundle generators.
//
// All four targets (Cloudflare / Vercel / Docker / Node) generate broadly the
// same shape: read a request, resolve the system prompt against env-derived
// template vars, dispatch to the customer's chosen LLM provider, optionally
// forward a trace, return JSON. The provider call snippets and template-var
// machinery live here so the per-target files focus on framework-specific
// concerns (Worker env, Edge runtime, Express, plain HTTP).

import type { Agent } from "@/lib/agents";

export type DeployProvider =
  | "anthropic"
  | "openai"
  | "gemini"
  | "groq"
  | "mistral"
  | "deepseek"
  | "xai"
  | "together"
  | "fireworks";

export interface DeployBundleConfig {
  brandName: string;
  allowedOrigins: string[];
  provider: DeployProvider;
  model?: string;
  forwardTraces: boolean;
  /** v2.0 Wave 2 — when true, the generator inlines the agent's local
   *  knowledge chunks into the deployed bundle, and the bundle does
   *  cosine-similarity retrieval per-request before calling the LLM. */
  useKnowledge: boolean;
}

export interface InlineKnowledgeChunk {
  /** source filename — shown in the <context> block as a header. */
  s: string;
  /** chunk content. */
  c: string;
  /** 1536 floats from text-embedding-3-small. */
  e: number[];
}

export const DEFAULT_DEPLOY_CONFIG: DeployBundleConfig = {
  brandName: "Support",
  allowedOrigins: ["https://example.com"],
  provider: "anthropic",
  forwardTraces: false,
  useKnowledge: false,
};

export interface GeneratedBundle {
  /** Path → contents. Customer drops these into a fresh project. */
  files: Record<string, string>;
  /** Shell commands to run after writing the files. */
  postInstall: string[];
}

export const PROVIDER_LABELS: Record<DeployProvider, string> = {
  anthropic:  "Anthropic Claude",
  openai:     "OpenAI GPT",
  gemini:     "Google Gemini",
  groq:       "Groq",
  mistral:    "Mistral",
  deepseek:   "DeepSeek",
  xai:        "xAI Grok",
  together:   "Together AI",
  fireworks:  "Fireworks",
};

const OPENAI_COMPAT_URLS: Partial<Record<DeployProvider, string>> = {
  openai:    "https://api.openai.com/v1/chat/completions",
  groq:      "https://api.groq.com/openai/v1/chat/completions",
  mistral:   "https://api.mistral.ai/v1/chat/completions",
  deepseek:  "https://api.deepseek.com/v1/chat/completions",
  xai:       "https://api.x.ai/v1/chat/completions",
  together:  "https://api.together.xyz/v1/chat/completions",
  fireworks: "https://api.fireworks.ai/inference/v1/chat/completions",
};

/** Pull `{var}` tokens out of the system prompt. Each becomes an env var. */
export function extractTemplateVars(systemPrompt: string | null): string[] {
  if (!systemPrompt) return [];
  const matches = systemPrompt.match(/\{([a-zA-Z_][a-zA-Z0-9_]*)\}/g) ?? [];
  return Array.from(new Set(matches.map((m) => m.slice(1, -1))));
}

export function envVarName(templateVar: string): string {
  return `AGENT_VAR_${templateVar.toUpperCase()}`;
}

/** Resolve {var} tokens at runtime from a `read(name) -> string` lookup.
 *  We emit this same helper into every bundle (Worker / Edge / Node). */
export const RESOLVE_PROMPT_HELPER = `function resolveSystemPrompt(template, lookup, vars) {
  let out = template;
  for (const name of vars) {
    const value = lookup("AGENT_VAR_" + name.toUpperCase()) ?? "";
    out = out.replaceAll("{" + name + "}", value);
  }
  return out;
}`;

/** RAG retrieval helper. Embeds the user query via OpenAI, scores it
 *  against the inlined chunk embeddings via cosine similarity, returns the
 *  top-K chunks formatted as a <context> block. The bundle prepends this
 *  to the user message before the LLM call. */
export const RETRIEVE_KNOWLEDGE_HELPER = `async function retrieveKnowledgeContext(query, embedKey, chunks, k) {
  if (!embedKey || !chunks || chunks.length === 0 || !query) return "";
  // Embed the query — same model used at ingest time so vectors live in
  // the same space (text-embedding-3-small, 1536 dims).
  const r = await fetch("https://api.openai.com/v1/embeddings", {
    method: "POST",
    headers: { "Authorization": "Bearer " + embedKey, "content-type": "application/json" },
    body: JSON.stringify({ model: "text-embedding-3-small", input: query }),
  });
  if (!r.ok) {
    // Don't break the agent on an embedding outage — proceed without RAG.
    return "";
  }
  const data = await r.json();
  const q = data?.data?.[0]?.embedding;
  if (!q) return "";
  // Cosine sim against every chunk. For the bundle sizes we ship (~50
  // chunks max) this is fast enough at request time. Larger corpora
  // belong in a real vector DB — Pro feature in v2.1.
  let qNorm = 0;
  for (let i = 0; i < q.length; i++) qNorm += q[i] * q[i];
  qNorm = Math.sqrt(qNorm);
  const scored = [];
  for (const c of chunks) {
    let dot = 0, cn = 0;
    for (let i = 0; i < q.length; i++) {
      dot += q[i] * c.e[i];
      cn += c.e[i] * c.e[i];
    }
    const denom = qNorm * Math.sqrt(cn);
    scored.push({ c: c, score: denom === 0 ? 0 : dot / denom });
  }
  scored.sort((a, b) => b.score - a.score);
  const top = scored.slice(0, k);
  if (top.length === 0) return "";
  const blocks = top.map(s => "## " + s.c.s + "\\n\\n" + s.c.c).join("\\n\\n---\\n\\n");
  return "<context>\\n" + blocks + "\\n</context>\\n\\n";
}`;

/** Compact JSON serializer for the inlined chunk array. Floats are rounded
 *  to 6 decimal places — plenty of precision for cosine similarity, cuts
 *  the size of a 1536-dim vector by ~30% vs default JSON.stringify. */
export function serializeInlineChunks(chunks: InlineKnowledgeChunk[]): string {
  const rows = chunks.map((chunk) => {
    const e = chunk.e.map((f) => Number(f.toFixed(6)));
    return JSON.stringify({ s: chunk.s, c: chunk.c, e });
  });
  return "[\n  " + rows.join(",\n  ") + "\n]";
}

/** Build the per-provider fetch snippet. Returns JS source that assigns the
 *  assistant text to a variable named `response` after awaiting the call. */
export function renderProviderCall(provider: DeployProvider, model: string, apiKeyExpr: string): string {
  if (provider === "anthropic") {
    return `      const r = await fetch("https://api.anthropic.com/v1/messages", {
        method: "POST",
        headers: {
          "x-api-key": ${apiKeyExpr},
          "anthropic-version": "2023-06-01",
          "content-type": "application/json",
        },
        body: JSON.stringify({
          model: ${JSON.stringify(model)},
          system: systemPrompt,
          max_tokens: 1024,
          messages: [...history, { role: "user", content: userMessage }],
        }),
      });
      if (!r.ok) throw new Error("anthropic " + r.status + ": " + await r.text());
      const data = await r.json();
      response = data?.content?.[0]?.text ?? "";`;
  }
  if (provider === "gemini") {
    return `      const r = await fetch(
        "https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=" + ${apiKeyExpr},
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            systemInstruction: { parts: [{ text: systemPrompt }] },
            contents: [
              ...history.map(m => ({ role: m.role === "assistant" ? "model" : "user", parts: [{ text: m.content }] })),
              { role: "user", parts: [{ text: userMessage }] },
            ],
          }),
        },
      );
      if (!r.ok) throw new Error("gemini " + r.status + ": " + await r.text());
      const data = await r.json();
      response = data?.candidates?.[0]?.content?.parts?.[0]?.text ?? "";`;
  }
  const url = OPENAI_COMPAT_URLS[provider];
  if (!url) throw new Error(`unsupported provider: ${provider}`);
  return `      const r = await fetch(${JSON.stringify(url)}, {
        method: "POST",
        headers: {
          "Authorization": "Bearer " + ${apiKeyExpr},
          "content-type": "application/json",
        },
        body: JSON.stringify({
          model: ${JSON.stringify(model)},
          messages: [
            { role: "system", content: systemPrompt },
            ...history,
            { role: "user", content: userMessage },
          ],
          max_tokens: 1024,
        }),
      });
      if (!r.ok) throw new Error("provider " + r.status + ": " + await r.text());
      const data = await r.json();
      response = data?.choices?.[0]?.message?.content ?? "";`;
}

/** Standard banner block we emit at the top of each generated entrypoint. */
export function bannerComment(target: string, agent: Agent, config: DeployBundleConfig, templateVars: string[]): string {
  const lines = [
    `// Auto-generated by ATO v2.0.0 — ${target} for "${agent.displayName}".`,
    "//",
    "// Required environment variables:",
    "//   - PROVIDER_API_KEY       (your " + config.provider + " API key)",
  ];
  for (const v of templateVars) {
    lines.push(`//   - ${envVarName(v).padEnd(24)} (template variable {${v}})`);
  }
  if (config.useKnowledge) {
    lines.push("//   - EMBED_API_KEY          (OpenAI key for query embeddings — text-embedding-3-small)");
  }
  if (config.forwardTraces) {
    lines.push("//   - ATO_TRACE_KEY          (issued in ATO desktop → Deploy tab)");
  }
  lines.push("//", "// Origin allowlist is baked into the bundle below — re-deploy after edits.");
  return lines.join("\n");
}

export function chooseModel(agent: Agent, config: DeployBundleConfig): string {
  return config.model ?? agent.model ?? defaultModelForProvider(config.provider);
}

function defaultModelForProvider(provider: DeployProvider): string {
  switch (provider) {
    case "anthropic": return "claude-sonnet-4-6";
    case "openai":    return "gpt-4.1";
    case "gemini":    return "gemini-2.0-flash-exp";
    case "groq":      return "llama-3.3-70b-versatile";
    case "mistral":   return "mistral-large-latest";
    case "deepseek":  return "deepseek-chat";
    case "xai":       return "grok-2-latest";
    case "together":  return "meta-llama/Llama-3.3-70B-Instruct-Turbo";
    case "fireworks": return "accounts/fireworks/models/llama-v3p3-70b-instruct";
  }
}

export function jsonString(value: unknown): string {
  return JSON.stringify(value, null, 2);
}
