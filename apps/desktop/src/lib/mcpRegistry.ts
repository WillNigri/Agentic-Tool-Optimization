import fallbackJson from "@/data/mcp-registry-fallback.json";

// Registry of installable MCP servers (T4).
// Source order:
//   1. ato-cloud GET /mcp-registry (when signed in or public-readable)
//   2. Bundled fallback JSON
// Shape mirrors what's needed to write a runtime MCP config entry
// (.mcp.json / codex.json / .gemini/settings.json mcpServers key).

export type McpCategory = "files" | "data" | "communication" | "search" | "productivity" | "dev";

export type McpStdio = {
  type: "stdio";
  command: string;
  args: string[];
  env?: Record<string, string>;
};

export type McpHttp = {
  type: "http" | "sse";
  url: string;
};

export type McpInstall = McpStdio | McpHttp;

export type McpRegistryEntry = {
  id: string;
  name: string;
  description: string;
  category: McpCategory;
  install: McpInstall;
  authNote?: string;
  homepage?: string;
};

export type McpRegistry = {
  version: number;
  fetchedAt: string;
  entries: McpRegistryEntry[];
};

const fallback = fallbackJson as McpRegistry;

const REGISTRY_URL =
  (typeof import.meta !== "undefined" && (import.meta as ImportMeta & { env?: Record<string, string> }).env?.VITE_MCP_REGISTRY_URL) ||
  "https://api.agentictool.ai/api/mcp-registry";

let cached: McpRegistry | null = null;
let cachedAt = 0;
const TTL_MS = 60 * 60 * 1000; // 1h

export async function getMcpRegistry(): Promise<McpRegistry> {
  const now = Date.now();
  if (cached && now - cachedAt < TTL_MS) return cached;

  try {
    const res = await fetch(REGISTRY_URL, { method: "GET" });
    if (res.ok) {
      const body = (await res.json()) as { success?: boolean; data?: McpRegistry } | McpRegistry;
      // Cloud wraps `{success: true, data: McpRegistry}`; bundled file is the registry directly.
      const candidate =
        body && typeof body === "object" && "data" in body && body.data ? body.data : (body as McpRegistry);
      if (candidate && Array.isArray(candidate.entries)) {
        cached = candidate;
        cachedAt = now;
        return candidate;
      }
    }
  } catch {
    // network errors fall through to bundled fallback
  }

  cached = fallback;
  cachedAt = now;
  return fallback;
}

export const CATEGORY_LABEL: Record<McpCategory, string> = {
  files: "Files",
  data: "Data",
  communication: "Communication",
  search: "Search",
  productivity: "Productivity",
  dev: "Dev",
};

// Build the JSON snippet a user would paste into a runtime's MCP config.
// Used by both the registry installer and the custom-install form.
export function renderMcpConfigEntry(entry: McpRegistryEntry): string {
  const cfg = entry.install.type === "stdio"
    ? {
        command: entry.install.command,
        args: entry.install.args,
        ...(entry.install.env ? { env: entry.install.env } : {}),
      }
    : { url: entry.install.url };
  return JSON.stringify({ [entry.id]: cfg }, null, 2);
}

// v1.3.0 T4 follow-up — actually install the MCP into a runtime's config file
// via the Rust `install_mcp_server` command. Returns the path that was written.
export type InstallableRuntime = "claude" | "gemini" | "codex";

export const INSTALLABLE_RUNTIMES: InstallableRuntime[] = ["claude", "gemini", "codex"];

/**
 * Detect $VAR placeholders in stdio args / env.
 * Used to drive the install-time options form (filesystem paths, DATABASE_URL,
 * etc.) so the agent ends up with a fully scoped MCP rather than a literal
 * "$PROJECT_PATH" string in its config.
 */
export function detectPlaceholders(entry: McpRegistryEntry): string[] {
  const out = new Set<string>();
  if (entry.install.type !== "stdio") return [];
  const re = /\$([A-Z_][A-Z0-9_]*)/g;
  for (const arg of entry.install.args) {
    const matches = arg.matchAll(re);
    for (const m of matches) out.add(m[1]);
  }
  if (entry.install.env) {
    for (const v of Object.values(entry.install.env)) {
      const matches = v.matchAll(re);
      for (const m of matches) out.add(m[1]);
    }
  }
  return Array.from(out);
}

/** Substitute $VAR placeholders against a values map. */
function substitute(s: string, values: Record<string, string>): string {
  return s.replace(/\$([A-Z_][A-Z0-9_]*)/g, (_, name) => values[name] ?? `$${name}`);
}

/**
 * For MCPs that accept multiple positional path args (e.g. filesystem), the
 * registry entry has a single `$PROJECT_PATH` slot that we expand to N args
 * if the user picked multiple paths.
 */
function expandMultiPathArg(args: string[], values: Record<string, string | string[]>): string[] {
  const out: string[] = [];
  for (const a of args) {
    const m = a.match(/^\$([A-Z_][A-Z0-9_]*)$/);
    if (m && Array.isArray(values[m[1]])) {
      out.push(...(values[m[1]] as string[]));
    } else if (m && typeof values[m[1]] === "string") {
      out.push(values[m[1]] as string);
    } else {
      // mixed string with placeholders → simple substitution
      out.push(
        a.replace(/\$([A-Z_][A-Z0-9_]*)/g, (_, name) => {
          const v = values[name];
          return Array.isArray(v) ? (v[0] ?? `$${name}`) : (v ?? `$${name}`);
        })
      );
    }
  }
  return out;
}

export async function installMcpToRuntime(
  runtime: InstallableRuntime,
  entry: McpRegistryEntry,
  options?: { values?: Record<string, string | string[]> }
): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  const values = options?.values ?? {};
  const transport = entry.install.type;
  const payload =
    transport === "stdio"
      ? {
          name: entry.id,
          transport: "stdio",
          command: entry.install.command,
          args: expandMultiPathArg(entry.install.args, values),
          env: entry.install.env
            ? Object.fromEntries(
                Object.entries(entry.install.env).map(([k, v]) => [
                  k,
                  substitute(v, Object.fromEntries(
                    Object.entries(values).map(([key, val]) => [key, Array.isArray(val) ? val[0] ?? "" : val])
                  )),
                ])
              )
            : null,
          url: null,
        }
      : {
          name: entry.id,
          transport: entry.install.type,
          command: null,
          args: null,
          env: null,
          url: entry.install.url,
        };
  return invoke<string>("install_mcp_server", { runtime, entry: payload });
}

export async function installCustomMcp(
  runtime: InstallableRuntime,
  payload: {
    name: string;
    transport: "stdio" | "http" | "sse";
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    url?: string;
  }
): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("install_mcp_server", {
    runtime,
    entry: {
      name: payload.name,
      transport: payload.transport,
      command: payload.command ?? null,
      args: payload.args ?? null,
      env: payload.env ?? null,
      url: payload.url ?? null,
    },
  });
}
