import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X, Search, Copy, Check, ExternalLink, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  getMcpRegistry,
  renderMcpConfigEntry,
  installMcpToRuntime,
  installCustomMcp,
  INSTALLABLE_RUNTIMES,
  CATEGORY_LABEL,
  type McpRegistryEntry,
  type McpCategory,
  type InstallableRuntime,
} from "@/lib/mcpRegistry";

// T4 — Modal that lets the user install an MCP server.
// Two tabs:
//   - Registry: curated catalog + 1-click "install" (copies config snippet for now;
//     real Tauri write to runtime configs lands later in T4.b).
//   - Custom: manual stdio/http form.
//
// Today the modal renders the JSON snippet to paste into the runtime config and
// copies it to the clipboard. Full installation (writing to ~/.claude/mcp.json
// etc., streaming `npx` install output to the embedded terminal) lands when T5
// terminal + Tauri write commands are wired.

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function AddMcpModal({ open, onClose }: Props) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"registry" | "custom">("registry");

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-3xl max-h-[90vh] flex flex-col rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-center justify-between p-5 border-b border-cs-border">
          <div>
            <h2 className="text-lg font-semibold text-cs-text">
              {t("mcpManager.addTitle", "Add MCP server")}
            </h2>
            <p className="mt-1 text-xs text-cs-muted">
              {t(
                "mcpManager.addSubtitle",
                "Browse the registry or paste a custom command. ATO will give you the exact config snippet to drop into your runtime."
              )}
            </p>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={18} />
          </button>
        </header>

        <div className="px-5 pt-4 flex items-center gap-2">
          <Pill active={tab === "registry"} onClick={() => setTab("registry")}>
            {t("mcpManager.registry", "Registry")}
          </Pill>
          <Pill active={tab === "custom"} onClick={() => setTab("custom")}>
            {t("mcpManager.custom", "Custom")}
          </Pill>
        </div>

        <div className="flex-1 overflow-y-auto p-5">
          {tab === "registry" ? <RegistryTab /> : <CustomTab />}
        </div>
      </div>
    </div>
  );
}

function Pill({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-full px-3 py-1.5 text-xs font-medium transition",
        active
          ? "bg-cs-accent text-cs-bg"
          : "bg-cs-bg-raised text-cs-muted border border-cs-border hover:text-cs-text"
      )}
    >
      {children}
    </button>
  );
}

function RegistryTab() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<McpCategory | "all">("all");
  const [selected, setSelected] = useState<McpRegistryEntry | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["mcp-registry"],
    queryFn: getMcpRegistry,
    staleTime: 60 * 60 * 1000,
  });

  const entries = data?.entries ?? [];
  const categories = Array.from(new Set(entries.map((e) => e.category)));

  const filtered = entries.filter((e) => {
    if (category !== "all" && e.category !== category) return false;
    if (search) {
      const s = search.toLowerCase();
      return (
        e.name.toLowerCase().includes(s) ||
        e.description.toLowerCase().includes(s) ||
        e.id.toLowerCase().includes(s)
      );
    }
    return true;
  });

  if (selected) {
    return <RegistryDetail entry={selected} onBack={() => setSelected(null)} />;
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <div className="relative flex-1">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("mcpManager.searchPlaceholder", "Search registry…")}
            className="w-full rounded-lg border border-cs-border bg-cs-bg pl-9 pr-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
          />
        </div>
        {/* Linux WebKitGTK + dark themes can render <select> options with the
            system's default text color, ignoring our `text-cs-text` on the
            outer element. Felipe reported the dropdown appearing blank on
            Linux v1.5.20. Setting `[&>option]:bg-cs-bg-raised` +
            `[&>option]:text-cs-text` forces the option list to inherit our
            colors, so the text stays legible cross-platform. */}
        <select
          value={category}
          onChange={(e) => setCategory(e.target.value as McpCategory | "all")}
          className="rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none [&>option]:bg-cs-bg-raised [&>option]:text-cs-text"
        >
          <option value="all">{t("mcpManager.allCategories", "All categories")}</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {CATEGORY_LABEL[c]}
            </option>
          ))}
        </select>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center h-32">
          <Loader2 size={20} className="animate-spin text-cs-muted" />
        </div>
      ) : filtered.length === 0 ? (
        <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-sm text-cs-muted">
          {t("mcpManager.noResults", "No MCP servers match your search.")}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {filtered.map((entry) => (
            <button
              key={entry.id}
              type="button"
              onClick={() => setSelected(entry)}
              className="text-left rounded-lg border border-cs-border bg-cs-bg-raised p-3 hover:border-cs-hover transition"
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-sm font-medium text-cs-text">{entry.name}</span>
                <span className="text-[10px] uppercase tracking-wide text-cs-muted">
                  {CATEGORY_LABEL[entry.category]}
                </span>
              </div>
              <p className="mt-1.5 text-xs text-cs-muted line-clamp-2">{entry.description}</p>
              {entry.authNote && (
                <p className="mt-2 text-[10px] text-cs-warning">⚠ {entry.authNote}</p>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

const RUNTIME_LABEL: Record<InstallableRuntime, string> = {
  claude: "Claude Code",
  gemini: "Gemini CLI",
  codex: "Codex / OpenAI",
};

function RegistryDetail({
  entry,
  onBack,
}: {
  entry: McpRegistryEntry;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [copied, setCopied] = useState(false);
  const [installState, setInstallState] = useState<{
    runtime: InstallableRuntime | null;
    status: "idle" | "installing" | "ok" | "error";
    message?: string;
  }>({ runtime: null, status: "idle" });
  const snippet = renderMcpConfigEntry(entry);

  const copyToClipboard = async () => {
    try {
      await navigator.clipboard.writeText(snippet);
      setCopied(true);
    } catch {
      // ignore — fallback would be to select+copy programmatically
    }
  };

  const install = async (runtime: InstallableRuntime) => {
    setInstallState({ runtime, status: "installing" });
    try {
      const path = await installMcpToRuntime(runtime, entry);
      setInstallState({ runtime, status: "ok", message: path });
      // Refresh the MCP dashboard so the new server appears immediately
      // — Felipe had to switch tabs to see installed servers because the
      // local list was stale.
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
      queryClient.invalidateQueries({ queryKey: ["mcp-servers-with-tools"] });
    } catch (err) {
      setInstallState({
        runtime,
        status: "error",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  };

  useEffect(() => {
    if (!copied) return;
    const id = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(id);
  }, [copied]);

  return (
    <div className="space-y-4">
      <button
        type="button"
        onClick={onBack}
        className="text-xs text-cs-muted hover:text-cs-text inline-flex items-center gap-1"
      >
        ← {t("common.back", "Back")}
      </button>

      <div>
        <h3 className="text-base font-semibold text-cs-text">{entry.name}</h3>
        <p className="mt-1 text-sm text-cs-muted">{entry.description}</p>
        {entry.homepage && (
          <a
            href={entry.homepage}
            target="_blank"
            rel="noreferrer"
            className="mt-2 inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
          >
            {t("mcpManager.homepage", "Homepage")}
            <ExternalLink size={10} />
          </a>
        )}
      </div>

      {entry.authNote && (
        <div className="rounded-lg border border-cs-warning/40 bg-cs-warning/10 p-3 text-xs text-cs-text">
          ⚠ {entry.authNote}
        </div>
      )}

      <div>
        <span className="block text-xs font-medium text-cs-muted uppercase tracking-wide mb-2">
          {t("mcpManager.installInto", "Install into")}
        </span>
        <div className="flex flex-wrap gap-2">
          {INSTALLABLE_RUNTIMES.map((rt) => {
            const isThisOne = installState.runtime === rt;
            const installing = isThisOne && installState.status === "installing";
            const installed = isThisOne && installState.status === "ok";
            return (
              <button
                key={rt}
                type="button"
                onClick={() => install(rt)}
                disabled={installing}
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs font-medium transition",
                  installed
                    ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                    : "border-cs-border bg-cs-bg-raised text-cs-text hover:border-cs-hover",
                  installing && "opacity-60 cursor-wait"
                )}
              >
                {installing ? (
                  <Loader2 size={12} className="animate-spin" />
                ) : installed ? (
                  <Check size={12} />
                ) : null}
                {RUNTIME_LABEL[rt]}
              </button>
            );
          })}
        </div>
        {installState.status === "ok" && installState.message && (
          <p className="mt-2 text-[11px] text-cs-accent font-mono break-all">
            {t("mcpManager.installedTo", "Wrote to {{path}}", { path: installState.message })}
          </p>
        )}
        {installState.status === "error" && installState.message && (
          <p className="mt-2 text-[11px] text-cs-danger break-all">{installState.message}</p>
        )}
      </div>

      <div>
        <div className="flex items-center justify-between mb-1.5">
          <span className="text-xs font-medium text-cs-muted uppercase tracking-wide">
            {t("mcpManager.configSnippet", "Config snippet")}
          </span>
          <button
            type="button"
            onClick={copyToClipboard}
            className="inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
          >
            {copied ? <Check size={12} /> : <Copy size={12} />}
            {copied ? t("common.copied", "Copied") : t("common.copy", "Copy")}
          </button>
        </div>
        <pre className="rounded-lg border border-cs-border bg-cs-bg p-3 text-xs text-cs-text font-mono overflow-x-auto">
          {snippet}
        </pre>
        <p className="mt-2 text-xs text-cs-muted">
          {t(
            "mcpManager.installHint",
            "Paste this into the mcpServers section of your runtime config (.claude/mcp.json, .gemini/settings.json, etc.) and restart the runtime."
          )}
        </p>
      </div>
    </div>
  );
}

function CustomTab() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [transport, setTransport] = useState<"stdio" | "http">("stdio");
  const [command, setCommand] = useState("");
  const [argsRaw, setArgsRaw] = useState("");
  const [url, setUrl] = useState("");
  const [envRaw, setEnvRaw] = useState("");
  const [installState, setInstallState] = useState<{
    runtime: InstallableRuntime | null;
    status: "idle" | "installing" | "ok" | "error";
    message?: string;
  }>({ runtime: null, status: "idle" });

  const parsedArgs = argsRaw
    .split("\n")
    .map((a) => a.trim())
    .filter(Boolean);
  const parsedEnv: Record<string, string> = {};
  envRaw
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .forEach((line) => {
      const i = line.indexOf("=");
      if (i > 0) parsedEnv[line.slice(0, i)] = line.slice(i + 1);
    });

  const install = async (runtime: InstallableRuntime) => {
    if (!name.trim()) return;
    setInstallState({ runtime, status: "installing" });
    try {
      const path = await installCustomMcp(runtime, {
        name: name.trim(),
        transport,
        command: transport === "stdio" ? command : undefined,
        args: transport === "stdio" ? parsedArgs : undefined,
        env: transport === "stdio" && Object.keys(parsedEnv).length > 0 ? parsedEnv : undefined,
        url: transport !== "stdio" ? url : undefined,
      });
      setInstallState({ runtime, status: "ok", message: path });
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
      queryClient.invalidateQueries({ queryKey: ["mcp-servers-with-tools"] });
    } catch (err) {
      setInstallState({
        runtime,
        status: "error",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const snippet = (() => {
    if (!name.trim()) return "";
    if (transport === "stdio") {
      const cfg = {
        command: command || "",
        args: parsedArgs,
        ...(Object.keys(parsedEnv).length > 0 ? { env: parsedEnv } : {}),
      };
      return JSON.stringify({ [name]: cfg }, null, 2);
    } else {
      return JSON.stringify({ [name]: { url: url || "" } }, null, 2);
    }
  })();

  return (
    <div className="space-y-4">
      <Field label={t("mcpManager.name", "Name")}>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-mcp"
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
        />
      </Field>

      <Field label={t("mcpManager.transport", "Transport")}>
        <div className="flex gap-2">
          {(["stdio", "http"] as const).map((tt) => (
            <button
              key={tt}
              type="button"
              onClick={() => setTransport(tt)}
              className={cn(
                "rounded-md px-3 py-1.5 text-xs font-medium border transition",
                transport === tt
                  ? "bg-cs-accent/10 border-cs-accent text-cs-accent"
                  : "bg-cs-bg-raised border-cs-border text-cs-muted hover:text-cs-text"
              )}
            >
              {tt}
            </button>
          ))}
        </div>
      </Field>

      {transport === "stdio" ? (
        <>
          <Field label={t("mcpManager.command", "Command")}>
            <input
              type="text"
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              placeholder="npx"
              className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
            />
          </Field>
          <Field label={t("mcpManager.args", "Args (one per line)")}>
            <textarea
              value={argsRaw}
              onChange={(e) => setArgsRaw(e.target.value)}
              rows={3}
              placeholder={"-y\n@modelcontextprotocol/server-foo"}
              className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
            />
          </Field>
          <Field label={t("mcpManager.env", "Env (KEY=value, one per line)")}>
            <textarea
              value={envRaw}
              onChange={(e) => setEnvRaw(e.target.value)}
              rows={3}
              placeholder="API_TOKEN=sk-..."
              className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
            />
          </Field>
        </>
      ) : (
        <Field label={t("mcpManager.url", "URL")}>
          <input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://my-mcp.example.com/sse"
            className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
      )}

      {snippet && (
        <>
          <div>
            <span className="block text-xs font-medium text-cs-muted uppercase tracking-wide mb-2">
              {t("mcpManager.installInto", "Install into")}
            </span>
            <div className="flex flex-wrap gap-2">
              {INSTALLABLE_RUNTIMES.map((rt) => {
                const isThisOne = installState.runtime === rt;
                const installing = isThisOne && installState.status === "installing";
                const installed = isThisOne && installState.status === "ok";
                return (
                  <button
                    key={rt}
                    type="button"
                    disabled={!name.trim() || installing}
                    onClick={() => install(rt)}
                    className={cn(
                      "inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs font-medium transition",
                      installed
                        ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                        : "border-cs-border bg-cs-bg-raised text-cs-text hover:border-cs-hover",
                      (installing || !name.trim()) && "opacity-60 cursor-not-allowed"
                    )}
                  >
                    {installing ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : installed ? (
                      <Check size={12} />
                    ) : null}
                    {RUNTIME_LABEL[rt]}
                  </button>
                );
              })}
            </div>
            {installState.status === "ok" && installState.message && (
              <p className="mt-2 text-[11px] text-cs-accent font-mono break-all">
                {t("mcpManager.installedTo", "Wrote to {{path}}", { path: installState.message })}
              </p>
            )}
            {installState.status === "error" && installState.message && (
              <p className="mt-2 text-[11px] text-cs-danger break-all">{installState.message}</p>
            )}
          </div>
          <div>
            <div className="flex items-center justify-between mb-1.5">
              <span className="text-xs font-medium text-cs-muted uppercase tracking-wide">
                {t("mcpManager.configSnippet", "Config snippet")}
              </span>
            </div>
            <pre className="rounded-lg border border-cs-border bg-cs-bg p-3 text-xs text-cs-text font-mono overflow-x-auto">
              {snippet}
            </pre>
          </div>
        </>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-xs font-medium text-cs-muted uppercase tracking-wide mb-1">
        {label}
      </span>
      {children}
    </label>
  );
}
