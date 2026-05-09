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

// v2.0.0 Wave 3 — Docker bundle generator.
//
// Emits a Dockerfile (alpine + node 22) and a single-file Express server.
// The customer runs `docker build -t my-agent .` then deploys the image
// to whatever orchestrator they use (Railway, Render, Fly, ECS, k8s).
// Express + node:22-alpine is small (~120 MB) and the no-build path
// (require/import directly) keeps cold start fast.

export function generateDocker(
  agent: Agent,
  config: DeployBundleConfig,
  knowledgeChunks: InlineKnowledgeChunk[] = [],
): GeneratedBundle {
  const model = chooseModel(agent, config);
  const templateVars = extractTemplateVars(agent.systemPrompt);
  const systemPromptLiteral = jsonString(agent.systemPrompt ?? "You are a helpful assistant.");
  const allowedOriginsLiteral = jsonString(config.allowedOrigins);
  const callBlock = renderProviderCall(config.provider, model, "process.env.PROVIDER_API_KEY");
  const traceBlock = config.forwardTraces ? renderNodeTraceForward(agent) : "    // (trace forwarding disabled)";
  const useKnowledge = config.useKnowledge && knowledgeChunks.length > 0;
  const chunksLiteral = useKnowledge ? serializeInlineChunks(knowledgeChunks) : "[]";

  const indexJs = `${bannerComment("Docker Express server", agent, config, templateVars)}

import express from "express";

const SYSTEM_PROMPT_TEMPLATE = ${systemPromptLiteral};
const ALLOWED_ORIGINS = new Set(${allowedOriginsLiteral});
const TEMPLATE_VARS = ${jsonString(templateVars)};
const KNOWLEDGE_CHUNKS = ${chunksLiteral};
const PORT = parseInt(process.env.PORT || "8080", 10);

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

const app = express();
app.use(express.json({ limit: "1mb" }));

app.options("/agent", (req, res) => {
  const origin = req.get("Origin") ?? "";
  res.set(corsHeaders(origin)).status(204).end();
});

app.get("/health", (_req, res) => {
  res.json({ status: "ok", agent: ${JSON.stringify(agent.slug)} });
});

app.post("/agent", async (req, res) => {
  const origin = req.get("Origin") ?? "";
  if (!ALLOWED_ORIGINS.has(origin)) {
    return res.set(corsHeaders(origin)).status(403).json({ error: "origin not allowed" });
  }

  let userMessage = String(req.body?.message ?? "").slice(0, 8000);
  const history = Array.isArray(req.body?.history) ? req.body.history.slice(-20) : [];
  const systemPrompt = resolveSystemPrompt(SYSTEM_PROMPT_TEMPLATE, (n) => process.env[n], TEMPLATE_VARS);
  const startedAt = Date.now();

  if (KNOWLEDGE_CHUNKS.length > 0) {
    const ctx = await retrieveKnowledgeContext(userMessage, process.env.EMBED_API_KEY, KNOWLEDGE_CHUNKS, 5);
    userMessage = ctx + userMessage;
  }

  let response;
  try {
${callBlock}
  } catch (err) {
    return res.set(corsHeaders(origin)).status(502).json({ error: String(err) });
  }

${traceBlock}

  res.set(corsHeaders(origin)).json({ message: response, latencyMs: Date.now() - startedAt });
});

app.listen(PORT, () => {
  console.log("[" + ${JSON.stringify(agent.slug)} + "] listening on :" + PORT);
});
`;

  const packageJson = jsonString({
    name: agent.slug,
    version: "1.0.0",
    type: "module",
    main: "index.js",
    scripts: {
      start: "node index.js",
    },
    dependencies: {
      express: "^4.21.0",
    },
    engines: {
      node: ">=20",
    },
  });

  // Multi-stage Dockerfile — install deps in builder, copy node_modules into a
  // slim runtime image. Smaller end image, faster cold pulls. Pinning the
  // alpine SHA keeps the build reproducible.
  const dockerfile = `# Multi-stage so the final image doesn't include the build cache.
FROM node:22-alpine AS deps
WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev

FROM node:22-alpine AS runtime
WORKDIR /app
ENV NODE_ENV=production
ENV PORT=8080
COPY --from=deps /app/node_modules ./node_modules
COPY index.js ./
COPY package.json ./
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s CMD wget -qO- http://127.0.0.1:8080/health || exit 1
CMD ["node", "index.js"]
`;

  const dockerignore = `node_modules
npm-debug.log
.env*
README*
.git*
`;

  const envTemplate = [
    `# Required by the Docker container at runtime.`,
    `# Pass via \`docker run -e PROVIDER_API_KEY=...\` or your orchestrator's secrets.`,
    `PROVIDER_API_KEY=`,
    ...templateVars.map((v) => `${envVarName(v)}=`),
    ...(useKnowledge ? ["EMBED_API_KEY="] : []),
    ...(config.forwardTraces ? ["ATO_TRACE_KEY="] : []),
  ].join("\n") + "\n";

  // v2.0.0 Wave 4 — emit the embed widget files for a working chat bubble.
  // Docker bundles host them under static/ — but Docker is backend-only,
  // so the embed files are mainly reference for the customer to copy to
  // their actual frontend.
  const embedFiles = generateEmbedFiles(agent, config);

  return {
    files: {
      "Dockerfile": dockerfile,
      ".dockerignore": dockerignore,
      "index.js": indexJs,
      "package.json": packageJson,
      ".env.example": envTemplate,
      ...Object.fromEntries(
        Object.entries(embedFiles).map(([name, content]) => [`embed/${name}`, content]),
      ),
    },
    postInstall: [
      `docker build -t ${agent.slug} .`,
      `docker run -p 8080:8080 \\`,
      `  -e PROVIDER_API_KEY=$PROVIDER_API_KEY \\`,
      ...templateVars.map((v) => `  -e ${envVarName(v)}=$${envVarName(v)} \\`),
      ...(useKnowledge ? [`  -e EMBED_API_KEY=$EMBED_API_KEY \\`] : []),
      ...(config.forwardTraces ? [`  -e ATO_TRACE_KEY=$ATO_TRACE_KEY \\`] : []),
      `  ${agent.slug}`,
    ],
  };
}

function renderNodeTraceForward(agent: Agent): string {
  return `    // Best-effort trace forwards — ATO Insights + Langfuse + generic
    // webhook all run in parallel, each gated on its own env var.
    if (process.env.ATO_TRACE_KEY) {
      // v2.1.0 — /embed sub-path with canonical batched payload.
      // v2.1 Phase 10 — forward embedSession when the widget supplied it.
      const __es = (payload && typeof payload === "object" && payload.embedSession && typeof payload.embedSession === "object") ? payload.embedSession : null;
      fetch("https://api.agentictool.ai/api/agent-traces/embed", {
        method: "POST",
        headers: { "Authorization": "Bearer " + process.env.ATO_TRACE_KEY, "content-type": "application/json" },
        body: JSON.stringify({
          traces: [{
            agentSlug: ${JSON.stringify(agent.slug)},
            runtime: "external",
            startedAt: new Date(startedAt).toISOString(),
            durationMs: Date.now() - startedAt,
            ok: true,
            source: "embed-docker",
            metadata: __es ? { origin, embedSession: __es } : { origin },
          }],
        }),
      }).catch(() => {});
    }
    forwardLangfuse(process.env, ${JSON.stringify(agent.slug)}, userMessage, response, Date.now() - startedAt, origin);
    forwardWebhook(process.env, ${JSON.stringify(agent.slug)}, userMessage, response, Date.now() - startedAt, origin);`;
}
