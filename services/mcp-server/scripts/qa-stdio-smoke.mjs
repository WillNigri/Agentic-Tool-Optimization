// QA §5 — minimal stdio MCP client smoke test.
//
// Spawns the built MCP server (services/mcp-server/dist/index.js),
// drives the Model Context Protocol handshake, lists tools, invokes
// a small read-only tool, and asserts the response shape. Catches the
// class of "tool registered but argv is wrong / handler crashes on
// first call" bugs that the static parity-check in §2 misses.
//
// Usage:
//   cd services/mcp-server && npm run build && node scripts/qa-stdio-smoke.mjs

import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const serverEntry = path.resolve(__dirname, "..", "dist", "index.js");

// In dev, ato may not be on PATH (the user hasn't run `ato setup-path`).
// Point the MCP server at the debug binary so the shell-out tools can
// actually run. In a real deploy this is unnecessary — users install
// ATO and run setup-path or the .app's binary lands in the sidecar
// path the MCP server probes anyway.
const repoRoot = path.resolve(__dirname, "..", "..", "..");
const devAtoBinary = path.join(repoRoot, "apps", "cli", "target", "debug", "ato");
const childEnv = { ...process.env };
if (!process.env.ATO_CLI_PATH) {
  childEnv.ATO_CLI_PATH = devAtoBinary;
}

const child = spawn("node", [serverEntry], {
  stdio: ["pipe", "pipe", "inherit"],
  env: childEnv,
});

let buf = "";
const pending = new Map(); // id → {resolve, reject}
let nextId = 1;

child.stdout.on("data", (chunk) => {
  buf += chunk.toString("utf8");
  // MCP JSON-RPC over stdio: newline-delimited JSON messages.
  let nl;
  while ((nl = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, nl);
    buf = buf.slice(nl + 1);
    if (!line.trim()) continue;
    let msg;
    try {
      msg = JSON.parse(line);
    } catch (e) {
      console.error("non-JSON stdout line:", line);
      continue;
    }
    if (msg.id != null && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(JSON.stringify(msg.error)));
      else resolve(msg.result);
    }
  }
});

function call(method, params = {}) {
  const id = nextId++;
  const payload = JSON.stringify({ jsonrpc: "2.0", id, method, params });
  child.stdin.write(payload + "\n");
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        reject(new Error(`MCP call ${method} timed out after 10s`));
      }
    }, 10_000);
  });
}

async function notify(method, params = {}) {
  const payload = JSON.stringify({ jsonrpc: "2.0", method, params });
  child.stdin.write(payload + "\n");
  // Notifications have no id and no response — wait a tick for the
  // server to process before issuing the next request.
  await new Promise((r) => setTimeout(r, 50));
}

const results = { passed: 0, failed: 0, errors: [] };
function assert(cond, label) {
  if (cond) {
    results.passed++;
    console.log("  ✓", label);
  } else {
    results.failed++;
    results.errors.push(label);
    console.log("  ✗", label);
  }
}

try {
  // 1. Initialize handshake.
  console.log("[init]");
  const init = await call("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "qa-stdio-smoke", version: "1.0.0" },
  });
  assert(init?.serverInfo?.name === "ato", "server identifies as 'ato'");
  assert(typeof init?.serverInfo?.version === "string", "server reports version");
  await notify("notifications/initialized");

  // 2. List tools — sanity-check the count and a few names.
  console.log("[tools/list]");
  const list = await call("tools/list");
  const tools = list?.tools ?? [];
  assert(tools.length >= 40, `tool count >= 40 (got ${tools.length})`);
  const names = new Set(tools.map((t) => t.name));
  for (const must of [
    "runtime_health",
    "list_pending_approvals",
    "sessions_list",
    "ratchet_check",
    "list_recent_events",
  ]) {
    assert(names.has(must), `tools include '${must}'`);
  }

  // 3. Invoke a small read-only tool and verify the shape.
  console.log("[tools/call runtime_health]");
  const health = await call("tools/call", {
    name: "runtime_health",
    arguments: {},
  });
  assert(
    Array.isArray(health?.content) && health.content.length > 0,
    "runtime_health returns content[]",
  );
  const text = health?.content?.[0]?.text;
  assert(typeof text === "string", "runtime_health content[0].text is a string");
  let parsed;
  try {
    parsed = JSON.parse(text);
    assert(Array.isArray(parsed), "runtime_health text parses as JSON array");
    if (Array.isArray(parsed) && parsed.length > 0) {
      const r = parsed[0];
      assert(typeof r.runtime === "string", "runtime_health row has 'runtime'");
      assert(typeof r.status === "string", "runtime_health row has 'status'");
    }
  } catch (e) {
    assert(false, `runtime_health text is valid JSON (got: ${e.message})`);
  }

  // 4. Invoke another tool to confirm consecutive calls work
  //    (catches state-leak between handlers).
  console.log("[tools/call list_pending_approvals]");
  const approvals = await call("tools/call", {
    name: "list_pending_approvals",
    arguments: {},
  });
  assert(
    Array.isArray(approvals?.content) && approvals.content.length > 0,
    "list_pending_approvals returns content[]",
  );
} catch (e) {
  console.error("FATAL:", e.message);
  results.failed++;
  results.errors.push(`FATAL: ${e.message}`);
} finally {
  child.kill();
}

console.log("\n=== Summary ===");
console.log(`passed: ${results.passed}`);
console.log(`failed: ${results.failed}`);
if (results.errors.length) {
  console.log("failures:");
  for (const e of results.errors) console.log("  -", e);
}
process.exit(results.failed === 0 ? 0 : 1);
