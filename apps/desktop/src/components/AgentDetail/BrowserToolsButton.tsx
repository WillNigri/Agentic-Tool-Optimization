import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Globe, Loader2, Check, AlertCircle } from "lucide-react";
import { installCustomMcp, type InstallableRuntime } from "@/lib/mcpRegistry";
import { getMcpServers } from "@/lib/api";
import { updateAgentMcps, type Agent } from "@/lib/agents";
import { cn } from "@/lib/utils";

// v1.5.0 Polish #1 — One-click "Add browser tools" button on Agent detail.
//
// Installs Playwright MCP for the agent's runtime if it isn't already
// installed, then attaches it to the agent's mcps list. Reuses the same
// install pipeline as the wizard's MCP install — runs through the existing
// hash-check + auto-backup + audit-log path. Idempotent: if Playwright is
// already installed and already attached, the button just confirms.

interface Props {
  agent: Agent;
}

const PLAYWRIGHT_MCP_NAME = "playwright";
const INSTALLABLE_RUNTIMES: InstallableRuntime[] = ["claude", "gemini", "codex"];

export default function BrowserToolsButton({ agent }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [state, setState] = useState<"idle" | "running" | "done" | "error">("idle");
  const [error, setError] = useState<string | null>(null);

  // Sequential and graph-only runtimes (openclaw, hermes) don't have a
  // local install path; the user has to wire Playwright manually for those.
  const supported = INSTALLABLE_RUNTIMES.includes(agent.runtime as InstallableRuntime);

  // Parse current MCPs (JSON array on the agent record).
  const currentMcps: string[] = (() => {
    if (!agent.mcps) return [];
    try {
      const v = JSON.parse(agent.mcps);
      return Array.isArray(v) ? (v as string[]) : [];
    } catch {
      return [];
    }
  })();
  const alreadyAttached = currentMcps.includes(PLAYWRIGHT_MCP_NAME);

  const handleClick = async () => {
    if (!supported || state === "running") return;
    setState("running");
    setError(null);
    try {
      // Install Playwright globally for this runtime if it's not already
      // present. We list MCPs and bail the install step if found — the
      // install pipeline writes to the runtime's config; double-writing
      // would just be a no-op but skipping is cleaner.
      const installed = await getMcpServers().catch(() => []);
      const alreadyInstalled = installed.some(
        (m) => m.name === PLAYWRIGHT_MCP_NAME ||
               m.name === `${PLAYWRIGHT_MCP_NAME} (claude)` ||
               m.name === `${PLAYWRIGHT_MCP_NAME} (codex)` ||
               m.name === `${PLAYWRIGHT_MCP_NAME} (gemini)`
      );
      if (!alreadyInstalled) {
        await installCustomMcp(agent.runtime as InstallableRuntime, {
          name: PLAYWRIGHT_MCP_NAME,
          transport: "stdio",
          command: "npx",
          args: ["-y", "@playwright/mcp@latest"],
        });
      }

      // Attach to the agent's mcps list if not already there.
      if (!alreadyAttached) {
        await updateAgentMcps(agent.id, [...currentMcps, PLAYWRIGHT_MCP_NAME]);
      }

      void queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
      void queryClient.invalidateQueries({ queryKey: ["mcp-servers-with-tools"] });
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["agent", agent.id] });
      setState("done");
      setTimeout(() => setState("idle"), 2500);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setState("error");
    }
  };

  if (!supported) {
    return (
      <div className="rounded-md border border-cs-border bg-cs-bg-raised/40 p-3 text-[11px] text-cs-muted">
        {t(
          "agentDetail.browserTools.unsupportedRuntime",
          "Browser tools auto-install isn't wired for {{runtime}} yet — install Playwright MCP manually under Skills & MCPs.",
          { runtime: agent.runtime }
        )}
      </div>
    );
  }

  return (
    <div className="rounded-md border border-cs-border bg-cs-bg-raised p-3 space-y-2">
      <div className="flex items-start gap-2">
        <Globe size={14} className="text-cs-accent shrink-0 mt-0.5" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium text-cs-text">
            {alreadyAttached
              ? t("agentDetail.browserTools.alreadyAttached", "Browser tools attached")
              : t("agentDetail.browserTools.title", "Browser tools")}
          </p>
          <p className="mt-0.5 text-[11px] text-cs-muted leading-relaxed">
            {alreadyAttached
              ? t(
                  "agentDetail.browserTools.alreadyAttachedHint",
                  "Playwright MCP is wired. The agent can navigate, click, type, and screenshot."
                )
              : t(
                  "agentDetail.browserTools.hint",
                  "One click installs Playwright MCP and attaches it. Agent gets browser_navigate, browser_click, browser_type, browser_screenshot, browser_snapshot."
                )}
          </p>
        </div>
        <button
          type="button"
          onClick={handleClick}
          disabled={state === "running" || alreadyAttached}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-[11px] font-medium shrink-0 transition-colors",
            alreadyAttached
              ? "border border-cs-accent/30 bg-cs-accent/5 text-cs-accent cursor-default"
              : state === "done"
              ? "border border-cs-accent/30 bg-cs-accent/10 text-cs-accent"
              : state === "error"
              ? "border border-cs-danger/40 bg-cs-danger/10 text-cs-danger hover:bg-cs-danger/15"
              : "bg-cs-accent text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
          )}
        >
          {state === "running" ? (
            <>
              <Loader2 size={11} className="animate-spin" />
              {t("agentDetail.browserTools.installing", "Installing…")}
            </>
          ) : state === "done" || alreadyAttached ? (
            <>
              <Check size={11} />
              {t("agentDetail.browserTools.attached", "Attached")}
            </>
          ) : state === "error" ? (
            <>
              <AlertCircle size={11} />
              {t("common.retry", "Retry")}
            </>
          ) : (
            <>
              <Globe size={11} />
              {t("agentDetail.browserTools.add", "Add browser tools")}
            </>
          )}
        </button>
      </div>
      {error && (
        <p className="text-[10px] text-cs-danger leading-relaxed pl-6">{error}</p>
      )}
    </div>
  );
}
