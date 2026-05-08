import type { Agent } from "@/lib/agents";
import {
  bannerComment,
  chooseModel,
  envVarName,
  extractTemplateVars,
  jsonString,
  RESOLVE_PROMPT_HELPER,
  RETRIEVE_KNOWLEDGE_HELPER,
  renderProviderCall,
  serializeInlineChunks,
  THIRD_PARTY_TRACE_FORWARDS,
  type DeployBundleConfig,
  type GeneratedBundle,
  type InlineKnowledgeChunk,
} from "./shared";
import { generateEmbedFiles } from "./embed";

// v2.0.0 Wave 3 — Vercel Edge Function generator.
//
// Emits a Next.js App Router edge route at `app/api/agent/route.ts`. The
// customer drops the generated files into a fresh `npx create-next-app` and
// runs `vercel deploy`. The Edge runtime supports the global `fetch` we use
// for provider dispatch, and `process.env` is available at build time +
// runtime (the customer sets vars in the Vercel dashboard or via
// `vercel env add`).

export function generateVercelEdge(
  agent: Agent,
  config: DeployBundleConfig,
  knowledgeChunks: InlineKnowledgeChunk[] = [],
): GeneratedBundle {
  const model = chooseModel(agent, config);
  const templateVars = extractTemplateVars(agent.systemPrompt);
  const systemPromptLiteral = jsonString(agent.systemPrompt ?? "You are a helpful assistant.");
  const allowedOriginsLiteral = jsonString(config.allowedOrigins);
  const callBlock = renderProviderCall(config.provider, model, "process.env.PROVIDER_API_KEY");
  const traceBlock = config.forwardTraces ? renderVercelTraceForward(agent) : "  // (trace forwarding disabled)";
  const useKnowledge = config.useKnowledge && knowledgeChunks.length > 0;
  const chunksLiteral = useKnowledge ? serializeInlineChunks(knowledgeChunks) : "[]";

  // .ts file so Next picks up types automatically. Using globalThis.fetch
  // and process.env keeps it compatible with both Edge and Node runtimes.
  const routeTs = `${bannerComment("Vercel Edge Function", agent, config, templateVars)}

export const runtime = "edge";

const SYSTEM_PROMPT_TEMPLATE = ${systemPromptLiteral};
const ALLOWED_ORIGINS = new Set<string>(${allowedOriginsLiteral});
const TEMPLATE_VARS = ${jsonString(templateVars)};
const KNOWLEDGE_CHUNKS: { s: string; c: string; e: number[] }[] = ${chunksLiteral};

${RESOLVE_PROMPT_HELPER}

${useKnowledge ? RETRIEVE_KNOWLEDGE_HELPER + "\n" : ""}

${THIRD_PARTY_TRACE_FORWARDS}

function corsHeaders(origin: string): Record<string, string> {
  const allowed = ALLOWED_ORIGINS.has(origin) ? origin : [...ALLOWED_ORIGINS][0] ?? "*";
  return {
    "Access-Control-Allow-Origin": allowed,
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "content-type",
    "Access-Control-Max-Age": "86400",
  };
}

export async function OPTIONS(request: Request): Promise<Response> {
  const origin = request.headers.get("Origin") ?? "";
  return new Response(null, { status: 204, headers: corsHeaders(origin) });
}

export async function POST(request: Request): Promise<Response> {
  const origin = request.headers.get("Origin") ?? "";
  if (!ALLOWED_ORIGINS.has(origin)) {
    return new Response(JSON.stringify({ error: "origin not allowed" }), {
      status: 403,
      headers: { ...corsHeaders(origin), "content-type": "application/json" },
    });
  }

  let payload: any;
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
  const systemPrompt = resolveSystemPrompt(SYSTEM_PROMPT_TEMPLATE, (n) => process.env[n], TEMPLATE_VARS);
  const startedAt = Date.now();

  if (KNOWLEDGE_CHUNKS.length > 0) {
    const ctx = await retrieveKnowledgeContext(userMessage, process.env.EMBED_API_KEY, KNOWLEDGE_CHUNKS, 5);
    userMessage = ctx + userMessage;
  }

  let response: string;
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
}
`;

  // Minimal vercel.json — just the runtime hint. The route file's `export
  // const runtime = "edge"` already declares it; we include this for
  // completeness so customers see exactly where the runtime is set.
  const vercelJson = jsonString({
    $schema: "https://openapi.vercel.sh/vercel.json",
    functions: {
      "app/api/agent/route.ts": {
        runtime: "edge",
      },
    },
  });

  // .env.local template so the customer knows the secret names without
  // hunting through the route source. They copy this and fill in values.
  const envTemplate = [
    `# Set these in the Vercel dashboard or via \`vercel env add\` for production.`,
    `# This file is for local development only — Next.js loads it automatically.`,
    `PROVIDER_API_KEY=`,
    ...templateVars.map((v) => `${envVarName(v)}=`),
    ...(useKnowledge ? ["EMBED_API_KEY="] : []),
    ...(config.forwardTraces ? ["ATO_TRACE_KEY="] : []),
  ].join("\n") + "\n";

  // v2.0.0 Wave 4 — emit the embed widget files for a working chat bubble.
  const embedFiles = generateEmbedFiles(agent, config);

  return {
    files: {
      "app/api/agent/route.ts": routeTs,
      "vercel.json": vercelJson,
      ".env.local.example": envTemplate,
      ...Object.fromEntries(
        Object.entries(embedFiles).map(([name, content]) => [`public/${name}`, content]),
      ),
    },
    postInstall: [
      "cp .env.local.example .env.local  # fill in the values",
      "vercel deploy",
      "# After deploy, set the same env vars in production:",
      "vercel env add PROVIDER_API_KEY production",
      ...templateVars.map((v) => `vercel env add ${envVarName(v)} production`),
      ...(useKnowledge ? ["vercel env add EMBED_API_KEY production"] : []),
      ...(config.forwardTraces ? ["vercel env add ATO_TRACE_KEY production"] : []),
    ],
  };
}

function renderVercelTraceForward(agent: Agent): string {
  // Edge runtime: no `ctx.waitUntil`. Use fetch().catch() and don't await —
  // the platform may cut us off after the response, which is fine for
  // best-effort telemetry.
  return `  // Best-effort trace forwards — fire and forget. ATO Insights +
  // Langfuse + generic webhook all run in parallel; each is gated on
  // its own env var so the customer opts in per-sink.
  if (process.env.ATO_TRACE_KEY) {
    fetch("https://api.agentictool.ai/api/agent-traces", {
      method: "POST",
      headers: { "Authorization": "Bearer " + process.env.ATO_TRACE_KEY, "content-type": "application/json" },
      body: JSON.stringify({
        agentSlug: ${JSON.stringify(agent.slug)},
        origin, userMessage, response,
        latencyMs: Date.now() - startedAt,
        timestamp: new Date().toISOString(),
      }),
    }).catch(() => {});
  }
  forwardLangfuse(process.env, ${JSON.stringify(agent.slug)}, userMessage, response, Date.now() - startedAt, origin);
  forwardWebhook(process.env, ${JSON.stringify(agent.slug)}, userMessage, response, Date.now() - startedAt, origin);`;
}
