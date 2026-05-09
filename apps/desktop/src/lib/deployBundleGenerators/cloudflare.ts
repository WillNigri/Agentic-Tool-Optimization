import type { Agent } from "@/lib/agents";
import {
  bannerComment,
  chooseModel,
  envVarName,
  extractTemplateVars,
  jsonString,
  RESOLVE_PROMPT_HELPER,
  RETRIEVE_KNOWLEDGE_HELPER,
  THIRD_PARTY_TRACE_FORWARDS,
  renderProviderCall,
  serializeInlineChunks,
  type DeployBundleConfig,
  type GeneratedBundle,
  type InlineKnowledgeChunk,
} from "./shared";
import { generateEmbedFiles } from "./embed";

// v2.0.0 Wave 1 — Cloudflare Worker code generator. See ./shared.ts for the
// pieces this shares with the other targets.
// Wave 2 — optional inlined knowledge for RAG retrieval.

export function generateCloudflareWorker(
  agent: Agent,
  config: DeployBundleConfig,
  knowledgeChunks: InlineKnowledgeChunk[] = [],
): GeneratedBundle {
  const model = chooseModel(agent, config);
  const templateVars = extractTemplateVars(agent.systemPrompt);
  const systemPromptLiteral = jsonString(agent.systemPrompt ?? "You are a helpful assistant.");
  const allowedOriginsLiteral = jsonString(config.allowedOrigins);
  const callBlock = renderProviderCall(config.provider, model, "env.PROVIDER_API_KEY");
  const traceBlock = config.forwardTraces ? renderTraceForward(agent) : "    // (trace forwarding disabled)";
  const useKnowledge = config.useKnowledge && knowledgeChunks.length > 0;
  const chunksLiteral = useKnowledge ? serializeInlineChunks(knowledgeChunks) : "[]";

  const workerJs = `${bannerComment("Cloudflare Worker", agent, config, templateVars)}

const SYSTEM_PROMPT_TEMPLATE = ${systemPromptLiteral};
const ALLOWED_ORIGINS = new Set(${allowedOriginsLiteral});
const TEMPLATE_VARS = ${jsonString(templateVars)};
const KNOWLEDGE_CHUNKS = ${chunksLiteral};

${RESOLVE_PROMPT_HELPER}

${useKnowledge ? RETRIEVE_KNOWLEDGE_HELPER + "\n" : ""}

${THIRD_PARTY_TRACE_FORWARDS}

function corsHeaders(origin) {
  const allowed = ALLOWED_ORIGINS.has(origin) ? origin : [...ALLOWED_ORIGINS][0] ?? "*";
  return {
    "Access-Control-Allow-Origin": allowed,
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "content-type",
    "Access-Control-Max-Age": "86400",
  };
}

export default {
  async fetch(request, env, ctx) {
    const origin = request.headers.get("Origin") ?? "";
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders(origin) });
    }
    if (request.method !== "POST") {
      return new Response(JSON.stringify({ error: "method not allowed" }), {
        status: 405,
        headers: { ...corsHeaders(origin), "content-type": "application/json" },
      });
    }
    if (!ALLOWED_ORIGINS.has(origin)) {
      return new Response(JSON.stringify({ error: "origin not allowed" }), {
        status: 403,
        headers: { ...corsHeaders(origin), "content-type": "application/json" },
      });
    }

    let payload;
    try {
      payload = await request.json();
    } catch {
      return new Response(JSON.stringify({ error: "invalid json" }), {
        status: 400,
        headers: { ...corsHeaders(origin), "content-type": "application/json" },
      });
    }

    let userMessage = String(payload?.message ?? "").slice(0, 8000);
    const history = Array.isArray(payload?.history) ? payload.history.slice(-20) : [];
    const systemPrompt = resolveSystemPrompt(SYSTEM_PROMPT_TEMPLATE, (n) => env[n], TEMPLATE_VARS);
    const startedAt = Date.now();

    // RAG: prepend top-K matching knowledge chunks to the user message.
    // No-op when KNOWLEDGE_CHUNKS is empty or EMBED_API_KEY isn't set —
    // we don't fail the request; the agent just runs without RAG.
    if (KNOWLEDGE_CHUNKS.length > 0) {
      const ctx = await retrieveKnowledgeContext(userMessage, env.EMBED_API_KEY, KNOWLEDGE_CHUNKS, 5);
      userMessage = ctx + userMessage;
    }

    let response;
    try {
${callBlock}
    } catch (err) {
      return new Response(JSON.stringify({ error: String(err) }), {
        status: 502,
        headers: { ...corsHeaders(origin), "content-type": "application/json" },
      });
    }

${traceBlock}

    return new Response(JSON.stringify({ message: response, latencyMs: Date.now() - startedAt }), {
      headers: { ...corsHeaders(origin), "content-type": "application/json" },
    });
  },
};
`;

  const wranglerToml = `name = "${agent.slug}"
main = "worker.js"
compatibility_date = "2025-01-01"

[vars]
# Non-secret config — edit and re-deploy. Secrets go via \`wrangler secret put\`.
${templateVars.length === 0 ? "# (no template variables — system prompt is static)" : templateVars.map((v) => `# ${envVarName(v)} = "..."`).join("\n")}
`;

  // v2.0.0 Wave 4 — also emit the embed widget files so the customer
  // gets a working chat-bubble alongside the backend bundle. embed.html
  // is a local test page; embed.js is the vanilla widget they paste a
  // <script> tag for on their real site.
  const embedFiles = generateEmbedFiles(agent, config);

  return {
    files: {
      "worker.js": workerJs,
      "wrangler.toml": wranglerToml,
      ...embedFiles,
    },
    postInstall: [
      "wrangler secret put PROVIDER_API_KEY",
      ...templateVars.map((v) => `wrangler secret put ${envVarName(v)}`),
      ...(useKnowledge ? ["wrangler secret put EMBED_API_KEY"] : []),
      ...(config.forwardTraces ? ["wrangler secret put ATO_TRACE_KEY"] : []),
      "wrangler deploy",
      "# After wrangler deploy prints your URL, edit embed.html's data-endpoint",
      "# to match it, then host embed.html + embed.js on your site.",
    ],
  };
}

function renderTraceForward(agent: Agent): string {
  // v2.1.0: hits /api/agent-traces/embed with the canonical batched
  // payload so the cloud route's existing schema accepts it. We send
  // metadata only — no userMessage / response content — because the
  // agent_traces table doesn't store those (PII), and there's no point
  // shipping bytes the server discards.
  // ATO Insights + Langfuse + generic webhook all run in parallel via
  // ctx.waitUntil so none of them block the user-facing response.
  return `    // Best-effort trace forwards — none of these block the user response.
    var atoTracePromise = env.ATO_TRACE_KEY
      ? fetch("https://api.agentictool.ai/api/agent-traces/embed", {
          method: "POST",
          headers: { "Authorization": "Bearer " + env.ATO_TRACE_KEY, "content-type": "application/json" },
          body: JSON.stringify({
            traces: [{
              agentSlug: ${JSON.stringify(agent.slug)},
              runtime: "external",
              startedAt: new Date(startedAt).toISOString(),
              durationMs: Date.now() - startedAt,
              ok: true,
              source: "embed-cloudflare",
              metadata: { origin: origin },
            }],
          }),
        }).catch(function () {})
      : Promise.resolve();
    ctx.waitUntil(Promise.all([
      atoTracePromise,
      forwardLangfuse(env, ${JSON.stringify(agent.slug)}, userMessage, response, Date.now() - startedAt, origin),
      forwardWebhook(env, ${JSON.stringify(agent.slug)}, userMessage, response, Date.now() - startedAt, origin),
    ]));`;
}

// Re-export shared types so existing imports keep working.
export type { DeployProvider, DeployBundleConfig, GeneratedBundle } from "./shared";
export { DEFAULT_DEPLOY_CONFIG } from "./shared";
