import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Bot,
  Trash2,
  Search,
  Loader2,
  AlertCircle,
  ChevronDown,
  ChevronRight,
  FileCode,
  Play,
  Zap,
  Settings2,
} from "lucide-react";
import { listAgents, deleteAgent, parseSkills, parseMcps, type Agent } from "@/lib/agents";
import { cn } from "@/lib/utils";
import RunAgentDialog from "./RunAgentDialog";
import AgentDetail from "@/components/AgentDetail/AgentDetail";
import { useTerminalStore } from "@/stores/useTerminalStore";
import { getRuntimeCapability, shellRequestForAgent } from "@/lib/runtimeCapabilities";

// v1.3.0 — User-created agents list (T3 follow-up).
// Shows agents that came from the Create Agent wizard (stored in the local
// SQLite agents table). Separate from SubagentsManager, which displays
// runtime-discovered built-in agents (Claude Code, OpenClaw, etc).

const RUNTIME_DOT: Record<Agent["runtime"], string> = {
  claude: "bg-orange-500",
  codex: "bg-green-500",
  gemini: "bg-blue-500",
  openclaw: "bg-cyan-400",
  hermes: "bg-purple-500",
};

const RUNTIME_LABEL: Record<Agent["runtime"], string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

export default function MyAgentsList() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const [runningAgent, setRunningAgent] = useState<Agent | null>(null);
  const [configuringAgent, setConfiguringAgent] = useState<Agent | null>(null);
  const requestShell = useTerminalStore((s) => s.requestShell);

  // Primary "Run" — open the embedded interactive shell scoped to this agent
  // so the user keeps memory across turns. Uses the per-runtime capability
  // matrix (Claude has @-mentions; Codex / Gemini get a prompt-prefix
  // fallback; OpenClaw / Hermes return null — the Run button is disabled).
  const runInShell = async (agent: Agent) => {
    const req = await shellRequestForAgent(agent.runtime, agent.slug);
    if (!req) return;
    requestShell(req.initialCommand, {
      followUpKeys: req.followUpKeys,
      followUpDelayMs: req.followUpDelayMs,
    });
  };

  const { data: agents = [], isLoading, error } = useQuery({
    queryKey: ["agents"],
    queryFn: () => listAgents(),
    staleTime: 5_000,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAgent(id, true),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["recent-agents"] });
      setPendingDelete(null);
    },
  });

  const filtered = search.trim()
    ? agents.filter(
        (a) =>
          a.displayName.toLowerCase().includes(search.toLowerCase()) ||
          a.slug.toLowerCase().includes(search.toLowerCase()) ||
          (a.description ?? "").toLowerCase().includes(search.toLowerCase())
      )
    : agents;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32">
        <Loader2 size={20} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-4 flex items-start gap-3">
        <AlertCircle size={16} className="text-cs-danger shrink-0 mt-0.5" />
        <div className="text-xs text-cs-text">
          {t("myAgents.loadError", "Couldn't load agents.")}{" "}
          <span className="font-mono text-cs-muted">
            {error instanceof Error ? error.message : String(error)}
          </span>
        </div>
      </div>
    );
  }

  if (agents.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-8 text-center">
        <Bot size={28} className="mx-auto text-cs-muted mb-3" />
        <h3 className="text-sm font-medium text-cs-text">
          {t("myAgents.emptyTitle", "No agents yet")}
        </h3>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "myAgents.emptyBody",
            'Use "+ New" to create your first agent. Built-in runtime agents (Claude Code subagents, OpenClaw gateway agents) are listed under the Built-in tab.'
          )}
        </p>
      </div>
    );
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
            placeholder={t("myAgents.searchPlaceholder", "Search agents…")}
            className="w-full rounded-lg border border-cs-border bg-cs-bg pl-9 pr-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
          />
        </div>
        <span className="text-xs text-cs-muted whitespace-nowrap">
          {t("myAgents.count", "{{count}} agent", { count: agents.length })}
          {agents.length !== 1 ? "s" : ""}
        </span>
      </div>

      <div className="space-y-2">
        {filtered.map((agent) => (
          <AgentRow
            key={agent.id}
            agent={agent}
            expanded={expandedId === agent.id}
            onToggle={() => setExpandedId(expandedId === agent.id ? null : agent.id)}
            pendingDelete={pendingDelete === agent.id}
            onRequestDelete={() => setPendingDelete(agent.id)}
            onCancelDelete={() => setPendingDelete(null)}
            onConfirmDelete={() => deleteMutation.mutate(agent.id)}
            deleting={deleteMutation.isPending && deleteMutation.variables === agent.id}
            onRun={() => runInShell(agent)}
            onQuickTest={() => setRunningAgent(agent)}
            onConfigure={() => setConfiguringAgent(agent)}
          />
        ))}
      </div>

      {runningAgent && (
        <RunAgentDialog
          agent={runningAgent}
          open={!!runningAgent}
          onClose={() => setRunningAgent(null)}
        />
      )}

      {configuringAgent && (
        <AgentDetail
          agent={configuringAgent}
          onClose={() => setConfiguringAgent(null)}
        />
      )}
    </div>
  );
}

function AgentRow({
  agent,
  expanded,
  onToggle,
  pendingDelete,
  onRequestDelete,
  onCancelDelete,
  onConfirmDelete,
  deleting,
  onRun,
  onQuickTest,
  onConfigure,
}: {
  agent: Agent;
  expanded: boolean;
  onToggle: () => void;
  pendingDelete: boolean;
  onRequestDelete: () => void;
  onCancelDelete: () => void;
  onConfirmDelete: () => void;
  deleting: boolean;
  onRun: () => void;
  onQuickTest: () => void;
  onConfigure: () => void;
}) {
  const { t } = useTranslation();
  const skills = parseSkills(agent);
  const mcps = parseMcps(agent);

  return (
    <div className="rounded-lg border border-cs-border bg-cs-card overflow-hidden">
      <div className="flex items-stretch">
        <button
          type="button"
          onClick={onToggle}
          data-demo-id={`agent-row-${agent.slug}`}
          className="flex-1 flex items-center gap-3 px-4 py-3 text-left hover:bg-cs-bg-raised transition min-w-0"
        >
          <span className={cn("inline-block w-2 h-2 rounded-full shrink-0", RUNTIME_DOT[agent.runtime])} />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium text-cs-text truncate">{agent.displayName}</span>
              <span className="text-[10px] uppercase tracking-wide text-cs-muted">
                {RUNTIME_LABEL[agent.runtime]}
              </span>
            </div>
            {agent.description && (
              <p className="text-xs text-cs-muted truncate mt-0.5">{agent.description}</p>
            )}
          </div>
          {agent.model && (
            <span className="text-[10px] font-mono text-cs-muted shrink-0">{agent.model}</span>
          )}
          {expanded ? (
            <ChevronDown size={14} className="text-cs-muted shrink-0" />
          ) : (
            <ChevronRight size={14} className="text-cs-muted shrink-0" />
          )}
        </button>
        {(() => {
          const cap = getRuntimeCapability(agent.runtime);
          const canRun = cap.invocation.kind !== "manual";
          return (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                if (canRun) onRun();
              }}
              disabled={!canRun}
              className={cn(
                "flex items-center gap-1.5 px-4 border-l border-cs-border text-xs font-medium",
                canRun
                  ? "bg-cs-bg-raised text-cs-accent hover:bg-cs-accent/10"
                  : "bg-cs-bg-raised text-cs-muted cursor-not-allowed opacity-50"
              )}
              title={
                canRun
                  ? "Open an interactive shell session for this agent"
                  : cap.invocation.kind === "manual"
                  ? cap.invocation.instructions
                  : "Not yet supported for this runtime"
              }
            >
              <Play size={12} />
              Run
            </button>
          );
        })()}
      </div>

      {expanded && (
        <div className="px-4 pb-4 pt-1 border-t border-cs-border space-y-3">
          {agent.systemPrompt && (
            <details className="text-xs">
              <summary className="cursor-pointer text-cs-muted hover:text-cs-text">
                {t("myAgents.viewSystemPrompt", "View system prompt")}
              </summary>
              <pre className="mt-2 rounded bg-cs-bg p-3 text-cs-text font-mono whitespace-pre-wrap">
                {agent.systemPrompt}
              </pre>
            </details>
          )}

          {agent.filePath && (
            <div className="flex items-center gap-2 text-xs text-cs-muted">
              <FileCode size={12} />
              <span className="font-mono truncate">{agent.filePath}</span>
            </div>
          )}

          {(skills.length > 0 || mcps.length > 0) && (
            <div className="flex flex-wrap gap-3 text-xs">
              {skills.length > 0 && (
                <div>
                  <span className="text-cs-muted">{t("myAgents.skills", "Skills")}:</span>{" "}
                  <span className="text-cs-text">{skills.join(", ")}</span>
                </div>
              )}
              {mcps.length > 0 && (
                <div>
                  <span className="text-cs-muted">{t("myAgents.mcps", "MCPs")}:</span>{" "}
                  <span className="text-cs-text">{mcps.join(", ")}</span>
                </div>
              )}
            </div>
          )}

          {agent.goal && (
            <p className="text-xs italic text-cs-muted">"{agent.goal}"</p>
          )}

          <div className="flex items-center justify-between pt-2 border-t border-cs-border">
            <div className="flex items-center gap-3">
              <span className="text-[10px] text-cs-muted">
                {t("myAgents.created", "Created {{date}}", {
                  date: new Date(agent.createdAt).toLocaleString(),
                })}
              </span>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onQuickTest();
                }}
                className="inline-flex items-center gap-1 text-[11px] text-cs-muted hover:text-cs-accent"
                title="Single-shot test, no memory between runs"
              >
                <Zap size={10} />
                {t("myAgents.quickTest", "Quick test")}
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onConfigure();
                }}
                data-demo-id={`agent-configure-${agent.slug}`}
                className="inline-flex items-center gap-1 text-[11px] text-cs-muted hover:text-cs-accent"
                title="Open the full agent detail editor"
              >
                <Settings2 size={10} />
                {t("myAgents.configure", "Configure")}
              </button>
            </div>
            {pendingDelete ? (
              <div className="flex items-center gap-2">
                <span className="text-xs text-cs-danger">
                  {t("myAgents.deleteConfirm", "Delete this agent?")}
                </span>
                <button
                  type="button"
                  onClick={onCancelDelete}
                  className="text-xs text-cs-muted hover:text-cs-text"
                  disabled={deleting}
                >
                  {t("common.cancel", "Cancel")}
                </button>
                <button
                  type="button"
                  onClick={onConfirmDelete}
                  disabled={deleting}
                  className="inline-flex items-center gap-1 rounded-md bg-cs-danger/20 border border-cs-danger/40 text-cs-danger px-2 py-1 text-xs"
                >
                  {deleting && <Loader2 size={10} className="animate-spin" />}
                  {t("myAgents.deleteConfirmYes", "Yes, delete")}
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={onRequestDelete}
                className="inline-flex items-center gap-1 text-xs text-cs-muted hover:text-cs-danger"
              >
                <Trash2 size={12} />
                {t("common.delete", "Delete")}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
