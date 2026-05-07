import type { Agent } from "@/lib/agents";
import {
  bannerComment,
  chooseModel,
  envVarName,
  extractTemplateVars,
  jsonString,
  RESOLVE_PROMPT_HELPER,
  renderProviderCall,
  type DeployBundleConfig,
  type GeneratedBundle,
} from "./shared";

// v2.0.0 Wave 1 — Cloudflare Worker code generator. See ./shared.ts for the
// pieces this shares with the other targets.

export function generateCloudflareWorker(
  agent: Agent,
  config: DeployBundleConfig,
): GeneratedBundle {
  const model = chooseModel(agent, config);
  const templateVars = extractTemplateVars(agent.systemPrompt);
  const systemPromptLiteral = jsonString(agent.systemPrompt ?? "You are a helpful assistant.");
  const allowedOriginsLiteral = jsonString(config.allowedOrigins);
  const callBlock = renderProviderCall(config.provider, model, "env.PROVIDER_API_KEY");
  const traceBlock = config.forwardTraces ? renderTraceForward(agent) : "    // (trace forwarding disabled)";

  const workerJs = `${bannerComment("Cloudflare Worker", agent, config, templateVars)}

const SYSTEM_PROMPT_TEMPLATE = ${systemPromptLiteral};
const ALLOWED_ORIGINS = new Set(${allowedOriginsLiteral});
const TEMPLATE_VARS = ${jsonString(templateVars)};

${RESOLVE_PROMPT_HELPER}

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

    const userMessage = String(payload?.message ?? "").slice(0, 8000);
    const history = Array.isArray(payload?.history) ? payload.history.slice(-20) : [];
    const systemPrompt = resolveSystemPrompt(SYSTEM_PROMPT_TEMPLATE, (n) => env[n], TEMPLATE_VARS);
    const startedAt = Date.now();

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

  return {
    files: {
      "worker.js": workerJs,
      "wrangler.toml": wranglerToml,
    },
    postInstall: [
      "wrangler secret put PROVIDER_API_KEY",
      ...templateVars.map((v) => `wrangler secret put ${envVarName(v)}`),
      ...(config.forwardTraces ? ["wrangler secret put ATO_TRACE_KEY"] : []),
      "wrangler deploy",
    ],
  };
}

function renderTraceForward(agent: Agent): string {
  return `    // Best-effort trace forward — never blocks the user response.
    if (env.ATO_TRACE_KEY) {
      ctx.waitUntil(
        fetch("https://api.agentictool.ai/api/agent-traces", {
          method: "POST",
          headers: {
            "Authorization": "Bearer " + env.ATO_TRACE_KEY,
            "content-type": "application/json",
          },
          body: JSON.stringify({
            agentSlug: ${JSON.stringify(agent.slug)},
            origin,
            userMessage,
            response,
            latencyMs: Date.now() - startedAt,
            timestamp: new Date().toISOString(),
          }),
        }).catch(() => {}),
      );
    }`;
}

// Re-export shared types so existing imports keep working.
export type { DeployProvider, DeployBundleConfig, GeneratedBundle } from "./shared";
export { DEFAULT_DEPLOY_CONFIG } from "./shared";
