import { useState, lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { X, Variable, Layers, Brain, Cpu, FileText, Zap, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Agent } from "@/lib/agents";
import ErrorBoundary from "@/components/ErrorBoundary";

// v1.4.0 — AgentDetail page (full-screen overlay).
//
// Opens when the user clicks "Configure" on an agent card. Hosts the v1.4
// tabs (Variables / Context / Memory / Models / Evaluators) plus an Overview
// that surfaces the agent's current config + file path.

const VariablesTab = lazy(() => import("./VariablesTab"));
const ContextTab = lazy(() => import("./ContextTab"));
const MemoryTab = lazy(() => import("./MemoryTab"));
const ModelsTab = lazy(() => import("./ModelsTab"));
const EvaluatorsTab = lazy(() => import("./EvaluatorsTab"));

interface Props {
  agent: Agent;
  onClose: () => void;
}

type TabId = "overview" | "variables" | "context" | "memory" | "models" | "evaluators";

export default function AgentDetail({ agent, onClose }: Props) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TabId>("variables");

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-4xl max-h-[90vh] flex flex-col rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-center justify-between p-5 border-b border-cs-border">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold text-cs-text truncate">
              {agent.displayName}
            </h2>
            <p className="text-[11px] text-cs-muted truncate">
              <code className="font-mono">@{agent.slug}</code>
              {" · "}
              {agent.runtime}
              {agent.model ? ` · ${agent.model}` : ""}
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

        <nav
          className="flex flex-wrap gap-1 px-5 pt-3 border-b border-cs-border"
          role="tablist"
        >
          <TabPill active={tab === "overview"} onClick={() => setTab("overview")} icon={<FileText size={12} />}>
            {t("agentDetail.tabs.overview", "Overview")}
          </TabPill>
          <TabPill active={tab === "variables"} onClick={() => setTab("variables")} icon={<Variable size={12} />}>
            {t("agentDetail.tabs.variables", "Variables")}
          </TabPill>
          <TabPill active={tab === "context"} onClick={() => setTab("context")} icon={<Layers size={12} />}>
            {t("agentDetail.tabs.context", "Context")}
          </TabPill>
          <TabPill active={tab === "memory"} onClick={() => setTab("memory")} icon={<Brain size={12} />}>
            {t("agentDetail.tabs.memory", "Memory")}
          </TabPill>
          <TabPill active={tab === "models"} onClick={() => setTab("models")} icon={<Cpu size={12} />}>
            {t("agentDetail.tabs.models", "Models")}
          </TabPill>
          <TabPill active={tab === "evaluators"} onClick={() => setTab("evaluators")} icon={<Zap size={12} />}>
            {t("agentDetail.tabs.evaluators", "Evaluators")}
          </TabPill>
        </nav>

        <div className="flex-1 overflow-y-auto p-5 min-h-0">
          <ErrorBoundary key={tab}>
            <Suspense
              fallback={
                <div className="flex items-center justify-center h-32">
                  <Loader2 size={20} className="animate-spin text-cs-muted" />
                </div>
              }
            >
              {tab === "overview" && <OverviewTab agent={agent} />}
              {tab === "variables" && <VariablesTab agent={agent} />}
              {tab === "context" && <ContextTab agent={agent} />}
              {tab === "memory" && <MemoryTab agent={agent} />}
              {tab === "models" && <ModelsTab agent={agent} />}
              {tab === "evaluators" && <EvaluatorsTab agent={agent} />}
            </Suspense>
          </ErrorBoundary>
        </div>
      </div>
    </div>
  );
}

function TabPill({
  active,
  onClick,
  icon,
  children,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      role="tab"
      aria-selected={active}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition",
        active
          ? "bg-cs-accent/10 text-cs-accent"
          : "text-cs-muted hover:bg-cs-border/40 hover:text-cs-text"
      )}
    >
      {icon}
      {children}
    </button>
  );
}

function OverviewTab({ agent }: { agent: Agent }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4 text-sm">
      <Detail label={t("agentDetail.overview.description", "Description")}>
        {agent.description ?? "—"}
      </Detail>
      <Detail label={t("agentDetail.overview.runtime", "Runtime")}>
        {agent.runtime}
      </Detail>
      <Detail label={t("agentDetail.overview.model", "Model")}>
        {agent.model ?? "—"}
      </Detail>
      <Detail label={t("agentDetail.overview.filePath", "File path")}>
        <code className="font-mono text-xs">{agent.filePath ?? "—"}</code>
      </Detail>
      <Detail label={t("agentDetail.overview.systemPrompt", "System prompt")}>
        <pre className="rounded-md bg-cs-bg p-3 text-xs text-cs-text font-mono whitespace-pre-wrap max-h-64 overflow-y-auto">
          {agent.systemPrompt ?? "(none)"}
        </pre>
      </Detail>
    </div>
  );
}

function Detail({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className="mt-1 text-cs-text">{children}</div>
    </div>
  );
}

