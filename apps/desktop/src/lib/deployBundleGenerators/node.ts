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

// v2.0.0 Wave 3 — Standalone Node script generator.
//
// Single-file deployable. No Express, no build step, just node + the
// built-in `http` module so the customer can `node server.js` it
// anywhere — Railway, Render, Fly, a bare VPS, even systemd. Useful for
// folks who don't want Docker layered on top.

export function generateNodeScript(
  agent: Agent,
  config: DeployBundleConfig,
): GeneratedBundle {
  const model = chooseModel(agent, config);
  const templateVars = extractTemplateVars(agent.systemPrompt);
  const systemPromptLiteral = jsonString(agent.systemPrompt ?? "You are a helpful assistant.");
  const allowedOriginsLiteral = jsonString(config.allowedOrigins);
  const callBlock = renderProviderCall(config.provider, model, "process.env.PROVIDER_API_KEY");
  const traceBlock = config.forwardTraces ? renderTraceForward(agent) : "    // (trace forwarding disabled)";

  const serverJs = `${bannerComment("standalone Node server", agent, config, templateVars)}

import http from "node:http";

const SYSTEM_PROMPT_TEMPLATE = ${systemPromptLiteral};
const ALLOWED_ORIGINS = new Set(${allowedOriginsLiteral});
const TEMPLATE_VARS = ${jsonString(templateVars)};
const PORT = parseInt(process.env.PORT || "8080", 10);

${RESOLVE_PROMPT_HELPER}

function corsHeaders(origin) {
  const allowed = ALLOWED_ORIGINS.has(origin) ? origin : [...ALLOWED_ORIGINS][0] ?? "*";
  return {
    "access-control-allow-origin": allowed,
    "access-control-allow-methods": "POST, OPTIONS",
    "access-control-allow-headers": "content-type",
    "access-control-max-age": "86400",
  };
}

function readJson(req) {
  return new Promise((resolve, reject) => {
    let data = "";
    req.on("data", (c) => { data += c; if (data.length > 1_048_576) reject(new Error("payload too large")); });
    req.on("end", () => { try { resolve(data ? JSON.parse(data) : {}); } catch (e) { reject(e); } });
    req.on("error", reject);
  });
}

async function handleAgent(req, res) {
  const origin = req.headers.origin ?? "";
  if (!ALLOWED_ORIGINS.has(origin)) {
    res.writeHead(403, { ...corsHeaders(origin), "content-type": "application/json" });
    res.end(JSON.stringify({ error: "origin not allowed" }));
    return;
  }

  let payload;
  try { payload = await readJson(req); } catch (e) {
    res.writeHead(400, { ...corsHeaders(origin), "content-type": "application/json" });
    res.end(JSON.stringify({ error: "invalid json", detail: String(e) }));
    return;
  }

  const userMessage = String(payload?.message ?? "").slice(0, 8000);
  const history = Array.isArray(payload?.history) ? payload.history.slice(-20) : [];
  const systemPrompt = resolveSystemPrompt(SYSTEM_PROMPT_TEMPLATE, (n) => process.env[n], TEMPLATE_VARS);
  const startedAt = Date.now();

  let response;
  try {
${callBlock}
  } catch (err) {
    res.writeHead(502, { ...corsHeaders(origin), "content-type": "application/json" });
    res.end(JSON.stringify({ error: String(err) }));
    return;
  }

${traceBlock}

  res.writeHead(200, { ...corsHeaders(origin), "content-type": "application/json" });
  res.end(JSON.stringify({ message: response, latencyMs: Date.now() - startedAt }));
}

const server = http.createServer(async (req, res) => {
  const origin = req.headers.origin ?? "";

  if (req.method === "OPTIONS") {
    res.writeHead(204, corsHeaders(origin));
    res.end();
    return;
  }

  if (req.method === "GET" && req.url === "/health") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ status: "ok", agent: ${JSON.stringify(agent.slug)} }));
    return;
  }

  if (req.method === "POST" && (req.url === "/agent" || req.url === "/")) {
    await handleAgent(req, res);
    return;
  }

  res.writeHead(404, { "content-type": "application/json" });
  res.end(JSON.stringify({ error: "not found" }));
});

server.listen(PORT, () => {
  console.log("[" + ${JSON.stringify(agent.slug)} + "] listening on :" + PORT);
});
`;

  const packageJson = jsonString({
    name: agent.slug,
    version: "1.0.0",
    type: "module",
    main: "server.js",
    scripts: {
      start: "node server.js",
    },
    engines: {
      node: ">=20",
    },
  });

  const envTemplate = [
    `# Set these in your environment before \`node server.js\`.`,
    `# Or set them in your deploy provider's dashboard (Railway / Render / Fly).`,
    `PROVIDER_API_KEY=`,
    ...templateVars.map((v) => `${envVarName(v)}=`),
    ...(config.forwardTraces ? ["ATO_TRACE_KEY="] : []),
    `# Optional — defaults to 8080`,
    `# PORT=8080`,
  ].join("\n") + "\n";

  // Tiny Procfile so Railway / Heroku / Fly auto-detect the start command.
  const procfile = "web: node server.js\n";

  return {
    files: {
      "server.js": serverJs,
      "package.json": packageJson,
      "Procfile": procfile,
      ".env.example": envTemplate,
    },
    postInstall: [
      "cp .env.example .env  # fill in the values, then `source .env`",
      "node server.js",
      "# To deploy:",
      "#   - Railway:  railway up",
      "#   - Render:   git push (configure web service to run `node server.js`)",
      "#   - Fly:      fly launch && fly deploy",
    ],
  };
}

function renderTraceForward(agent: Agent): string {
  return `    // Best-effort trace forward — fire and forget.
    if (process.env.ATO_TRACE_KEY) {
      fetch("https://api.agentictool.ai/api/agent-traces", {
        method: "POST",
        headers: {
          "Authorization": "Bearer " + process.env.ATO_TRACE_KEY,
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
      }).catch(() => {});
    }`;
}
