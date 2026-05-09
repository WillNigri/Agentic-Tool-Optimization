import { useState, lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X, Variable, Layers, Brain, Cpu, FileText, Zap, Loader2, Globe, Lock, BookOpen, Code2, History, TrendingDown } from "lucide-react";
import { getRegressions, type RegressionRow } from "@/lib/cloudAgentTraces";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { cn } from "@/lib/utils";
import type { Agent } from "@/lib/agents";
import { updateAgentKind } from "@/lib/agents";
import ErrorBoundary from "@/components/ErrorBoundary";
import BrowserToolsButton from "./BrowserToolsButton";

// v1.4.0 — AgentDetail page (full-screen overlay).
//
// Opens when the user clicks "Configure" on an agent card. Hosts the v1.4
// tabs (Variables / Context / Memory / Models / Evaluators) plus an Overview
// that surfaces the agent's current config + file path.
//
// v2.0.0 — Deploy tab visible only when agent.kind === 'external'. Header gets
// a kind badge + flip control.

const VariablesTab = lazy(() => import("./VariablesTab"));
const ContextTab = lazy(() => import("./ContextTab"));
const MemoryTab = lazy(() => import("./MemoryTab"));
const ModelsTab = lazy(() => import("./ModelsTab"));
const EvaluatorsTab = lazy(() => import("./EvaluatorsTab"));
const DeployTab = lazy(() => import("./DeployTab"));
const KnowledgeTab = lazy(() => import("./KnowledgeTab"));
const RawTab = lazy(() => import("./RawTab"));
const HistoryTab = lazy(() => import("./HistoryTab"));

interface Props {
  agent: Agent;
  onClose: () => void;
  /** v2.0.0 — initial tab to land on when the detail opens. Used by the
   *  Create Agent wizard's "Set up Knowledge & Deploy" CTA so the user
   *  drops directly into Knowledge after saving an external agent. */
  initialTab?: string | null;
}

type TabId = "overview" | "variables" | "context" | "memory" | "models" | "evaluators" | "deploy" | "knowledge" | "raw" | "history";

export default function AgentDetail({ agent, onClose, initialTab }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const isExternal = agent.kind === "external";
  const [tab, setTab] = useState<TabId>(() => {
    // Honor the caller's initialTab when it's a valid TabId, otherwise
    // default to "knowledge" for new external agents (v2.0.0 — landing
    // on Variables for an empty external agent felt like the wrong
    // first thing to show; Knowledge is where they actually start).
    const valid: TabId[] = [
      "overview", "variables", "context", "memory",
      "models", "evaluators", "deploy", "knowledge", "raw", "history",
    ];
    if (initialTab && (valid as string[]).includes(initialTab)) return initialTab as TabId;
    return "variables";
  });

  const flipKind = async () => {
    const next = isExternal ? "internal" : "external";
    await updateAgentKind(agent.id, next);
    queryClient.invalidateQueries({ queryKey: ["agents"] });
    queryClient.invalidateQueries({ queryKey: ["agent", agent.id] });
  };

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
            <h2 className="flex items-center gap-2 text-sm font-semibold text-cs-text truncate">
              <span className="truncate">{agent.displayName}</span>
              <button
                type="button"
                onClick={flipKind}
                className={cn(
                  "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium transition-colors",
                  isExternal
                    ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent hover:bg-cs-accent/20"
                    : "border-cs-border bg-cs-bg text-cs-muted hover:text-cs-text",
                )}
                title={t(
                  "agentDetail.kindFlipHint",
                  "Toggle internal/external. External unlocks the Deploy tab.",
                )}
              >
                {isExternal ? <Globe size={10} /> : <Lock size={10} />}
                {isExternal
                  ? t("agentDetail.kindExternal", "External")
                  : t("agentDetail.kindInternal", "Internal")}
              </button>
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
            data-demo-id="agent-detail-close"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={18} />
          </button>
        </header>

        {/* v2.1 Phase 5b — Regression banner. Surfaces an active
            regression on this agent so users see the warning without
            having to remember to check Insights → Regressions. */}
        <RegressionBanner agentSlug={agent.slug} />

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
          <TabPill active={tab === "context"} onClick={() => setTab("context")} icon={<Layers size={12} />} demoId="agent-tab-context">
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
          {isExternal && (
            <TabPill active={tab === "knowledge"} onClick={() => setTab("knowledge")} icon={<BookOpen size={12} />} demoId="agent-tab-knowledge">
              {t("agentDetail.tabs.knowledge", "Knowledge")}
            </TabPill>
          )}
          {isExternal && (
            <TabPill active={tab === "deploy"} onClick={() => setTab("deploy")} icon={<Globe size={12} />} demoId="agent-tab-deploy">
              {t("agentDetail.tabs.deploy", "Deploy")}
            </TabPill>
          )}
          <TabPill active={tab === "raw"} onClick={() => setTab("raw")} icon={<Code2 size={12} />} demoId="agent-tab-raw">
            {t("agentDetail.tabs.raw", "Raw")}
          </TabPill>
          <TabPill active={tab === "history"} onClick={() => setTab("history")} icon={<History size={12} />} demoId="agent-tab-history">
            {t("agentDetail.tabs.history", "History")}
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
              {tab === "deploy" && <DeployTab agent={agent} />}
              {tab === "knowledge" && <KnowledgeTab agent={agent} />}
              {tab === "raw" && <RawTab agent={agent} />}
              {tab === "history" && <HistoryTab agent={agent} />}
            </Suspense>
          </ErrorBoundary>
        </div>
      </div>
    </div>
  );
}

// v2.1 Phase 5b — small alert above the tabs when an active regression
// exists for this agent. Inline summary; users can hop to Insights →
// Regressions for the drill-down. Render-nothing on free / signed-out /
// no-data so the modal stays clean by default.
function RegressionBanner({ agentSlug }: { agentSlug: string }) {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const mock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  const canQuery = mock || (isCloudUser && !!accessToken);

  const query = useQuery({
    queryKey: ["regressions", 30],
    queryFn: () => getRegressions({ days: 30 }),
    enabled: !!canQuery && isPro,
    staleTime: 60_000,
  });

  if (!canQuery || !isPro) return null;
  const all = query.data?.regressions ?? [];
  // Most-recent regression for THIS agent only.
  const mine: RegressionRow | undefined = all
    .filter((r) => r.agent_slug === agentSlug && r.severity === "regression")
    .sort((a, b) => b.changed_at.localeCompare(a.changed_at))[0];
  if (!mine) return null;

  const okLine =
    mine.ok_delta_pp <= -10
      ? t("agentDetail.regressionBanner.okDrop", "ok rate {{n}}pp", {
          n: mine.ok_delta_pp.toFixed(1),
        })
      : null;
  const evalLine =
    mine.eval_delta_pp !== null && mine.eval_delta_pp <= -15
      ? t("agentDetail.regressionBanner.evalDrop", "eval score {{n}}pp", {
          n: mine.eval_delta_pp.toFixed(1),
        })
      : null;
  const p95Line =
    mine.p95_delta_pct >= 50
      ? t("agentDetail.regressionBanner.p95Up", "p95 latency +{{n}}%", {
          n: mine.p95_delta_pct.toFixed(0),
        })
      : null;
  const costLine =
    mine.cost_delta_pct >= 25
      ? t("agentDetail.regressionBanner.costUp", "cost +{{n}}%", {
          n: mine.cost_delta_pct.toFixed(0),
        })
      : null;
  const reasons = [okLine, evalLine, p95Line, costLine].filter(Boolean).join(" · ");

  return (
    <div className="border-b border-cs-danger/30 bg-cs-danger/5 px-5 py-2 text-[11px] text-cs-text flex items-start gap-2">
      <TrendingDown size={12} className="text-cs-danger shrink-0 mt-0.5" />
      <div className="min-w-0 flex-1">
        <span className="font-medium text-cs-danger">
          {t("agentDetail.regressionBanner.title", "Regression detected")}
        </span>{" "}
        <span className="text-cs-muted">
          {t("agentDetail.regressionBanner.body", "after the most recent {{field}} change", {
            field: mine.field,
          })}
          {reasons ? ` — ${reasons}` : ""}
          {". "}
          {t(
            "agentDetail.regressionBanner.see",
            "See Insights → Regressions for failing examples and rollback options.",
          )}
        </span>
      </div>
    </div>
  );
}

function TabPill({
  active,
  onClick,
  icon,
  children,
  demoId,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
  demoId?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-demo-id={demoId}
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

      <Detail label={t("agentDetail.overview.quickTools", "Quick tools")}>
        <BrowserToolsButton agent={agent} />
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

