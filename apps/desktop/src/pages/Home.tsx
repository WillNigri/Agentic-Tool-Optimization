import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Sparkles, Bot, Activity, AlertCircle, ArrowRight, Plus, Settings, Play } from "lucide-react";
import CreateAgentWizard, { type WizardPath } from "@/components/CreateAgentWizard";
import RuntimeHealthBanner from "@/components/RuntimeHealthBanner";
import RoiScanTile from "@/components/RoiScanTile/RoiScanTile";
import { queryAllAgentStatuses } from "@/lib/api";
import { listAgents, type Agent } from "@/lib/agents";
import RunAgentDialog from "@/components/MyAgentsList/RunAgentDialog";
import { useTerminalStore } from "@/stores/useTerminalStore";
import { useUiStore } from "@/stores/useUiStore";
import { shellRequestForAgent, getRuntimeCapability } from "@/lib/runtimeCapabilities";

// v1.3.0 — The GUI Pivot landing page (T1+T2+T9).
// Real data wired in T3: recent agents come from listAgents().
// First-run UX: detects "no runtime ready" and prompts the user to Settings → Runtimes.
// See docs/V1.3.0-IMPLEMENTATION.md.

type RecentAgent = {
  id: string;
  displayName: string;
  runtime: string;
  lastUsedAt: number | null;
};

type RecentRun = {
  id: string;
  agentName: string;
  runtime: string;
  status: "ok" | "error" | "running";
  startedAt: number;
};

type Alert = {
  id: string;
  severity: "danger" | "warning";
  message: string;
};

interface HomeProps {
  recentAgents?: RecentAgent[];
  recentRuns?: RecentRun[];
  alerts?: Alert[];
  onCreateAgent?: (path: WizardPath) => void;
  onOpenAgent?: (agentId: string) => void;
  onOpenRun?: (runId: string) => void;
  onOpenSettings?: () => void;
  /** PR-C (2026-05-21) — opens Insights from the ROI scan tile.
   *  Optional so tests can mount Home without wiring routing. */
  onOpenInsights?: () => void;
  /** Override runtime detection (used in tests). */
  runtimeReady?: boolean;
}

const RUNTIME_DOT: Record<RecentAgent["runtime"], string> = {
  claude: "bg-orange-500",
  codex: "bg-green-500",
  gemini: "bg-blue-500",
  openclaw: "bg-cyan-400",
  hermes: "bg-purple-500",
};

const STATUS_DOT: Record<RecentRun["status"], string> = {
  ok: "bg-cs-accent",
  error: "bg-cs-danger",
  running: "bg-cs-warning animate-pulse",
};

export default function Home({
  recentAgents,
  recentRuns = [],
  alerts = [],
  onCreateAgent,
  onOpenAgent,
  onOpenRun,
  onOpenSettings,
  onOpenInsights,
  runtimeReady,
}: HomeProps) {
  const { t } = useTranslation();
  const [hoveringQuick, setHoveringQuick] = useState(false);
  const [wizard, setWizard] = useState<WizardPath | null>(null);
  const [runningAgent, setRunningAgent] = useState<Agent | null>(null);

  // Cross-cutting wizard state — lets the demo runner / command palette /
  // anywhere else trigger the wizard with a chosen path.
  const uiCreateAgentOpen = useUiStore((s) => s.createAgentOpen);
  const uiCreateAgentPath = useUiStore((s) => s.createAgentPath);
  const closeUiCreateAgent = useUiStore((s) => s.closeCreateAgent);
  // PR-C — First-Chat Wizard. Home's primary CTA opens it; the wizard
  // is mounted globally in Dashboard so it works from any section.
  const openFirstChat = useUiStore((s) => s.openFirstChat);
  const requestShell = useTerminalStore((s) => s.requestShell);

  const runInShell = async (agent: Agent) => {
    const req = await shellRequestForAgent(agent.runtime, agent.slug);
    if (!req) return;
    requestShell(req.initialCommand, {
      followUpKeys: req.followUpKeys,
      followUpDelayMs: req.followUpDelayMs,
    });
  };

  const { data: statuses } = useQuery({
    queryKey: ["agent-statuses"],
    queryFn: queryAllAgentStatuses,
    enabled: runtimeReady === undefined,
    staleTime: 30_000,
  });

  // Recent agents from the agents table (T3). Skip fetching when caller supplies
  // their own list (used in tests).
  const { data: fetchedAgents } = useQuery({
    queryKey: ["recent-agents"],
    queryFn: () => listAgents(),
    enabled: recentAgents === undefined,
    staleTime: 10_000,
  });

  const effectiveRecentAgents: RecentAgent[] =
    recentAgents ?? (fetchedAgents ?? []).slice(0, 6).map(agentToRecent);

  const detectedReady = runtimeReady ?? (statuses ? statuses.some((s) => s.available) : true);

  const launch = (path: WizardPath) => {
    if (onCreateAgent) onCreateAgent(path);
    else setWizard(path);
  };

  const hasAgents = effectiveRecentAgents.length > 0;
  const hasRuns = recentRuns.length > 0;
  const hasAlerts = alerts.length > 0;

  return (
    <div className="max-w-5xl mx-auto space-y-8">
      <RuntimeHealthBanner />
      {!detectedReady && (
        <section className="flex items-start gap-3 rounded-lg border border-cs-warning/40 bg-cs-warning/10 p-4">
          <Settings size={18} className="text-cs-warning shrink-0" />
          <div className="flex-1">
            <h3 className="text-sm font-medium text-cs-text">
              {t("home.connectRuntimeTitle", "Connect a runtime to get started")}
            </h3>
            <p className="mt-1 text-xs text-cs-muted">
              {t(
                "home.connectRuntimeBody",
                "We didn't detect Claude, Codex, Gemini, OpenClaw, or Hermes on your machine. Open Settings → Runtimes to connect one (or use a stored API key)."
              )}
            </p>
          </div>
          {onOpenSettings && (
            <button
              type="button"
              onClick={onOpenSettings}
              className="rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
            >
              {t("home.openSettings", "Open Settings")}
            </button>
          )}
        </section>
      )}

      {/* Hero — local war room positioning (v2.4.7 final).
          Settled via a 5-round Gemini+MiniMax+human debate session itself —
          session id 1379b231-9d2b-4e06-a974-e9eb9217fbb6, recorded as live demo.
          Strategy: use-case-first marketing (war room) for 0-90 days, primitive
          narrative seeded 90-180 days under an ATO Core sub-brand.
          Use cases ride underneath the headline (strategy · pre-mortem ·
          architecture · code review · security audits) — code review is the
          most-validated, not the elevator pitch. */}
      <section className="rounded-2xl border border-cs-border bg-cs-card p-6 sm:p-8">
        {/* Stack vertically by default; only go side-by-side at >= md so the
            text never gets squeezed into one-word-per-line columns when the
            sidebar is wide or the window is narrow (v2.4.7 layout fix). */}
        <div className="flex flex-col md:flex-row md:items-start md:justify-between gap-6">
          <div className="flex-1 min-w-0">
            <h1 className="text-xl sm:text-2xl font-semibold text-cs-text leading-tight">
              {t(
                "home.heroTitle",
                "Your local war room for humans and LLMs."
              )}
            </h1>
            <p className="mt-2 text-sm sm:text-base text-cs-muted max-w-2xl">
              {t(
                "home.heroSubtitle",
                "Decide together, call real tools, walk out with a signed audit trail. Drop in any of your LLMs — Claude, GPT, Gemini, Grok, MiniMax, and 15+ more. Push back on them. Cite the repo. Ship the decision."
              )}
            </p>
            <p className="mt-3 text-xs text-cs-muted">
              {t(
                "home.heroUseCases",
                "Strategy debates · Pre-mortems · Architecture decisions · Code review · Security audits"
              )}
            </p>
          </div>
          {/* Buttons wrap to a row of their own on narrow viewports; stack
              vertically only when even one button doesn't fit per row. */}
          <div className="flex flex-wrap items-center gap-2 md:gap-3 md:flex-shrink-0">
            <button
              type="button"
              className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2.5 sm:px-5 sm:py-3 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover transition whitespace-nowrap"
              onClick={openFirstChat}
            >
              <Sparkles size={16} />
              {t("home.startGuided", "Start a war room")}
            </button>
            <button
              type="button"
              className="inline-flex items-center gap-2 rounded-lg border border-cs-border bg-cs-bg-raised px-4 py-2.5 sm:px-5 sm:py-3 text-sm font-medium text-cs-text hover:border-cs-hover transition whitespace-nowrap"
              onClick={() => launch("quick")}
              onMouseEnter={() => setHoveringQuick(true)}
              onMouseLeave={() => setHoveringQuick(false)}
            >
              <Plus size={16} />
              {t("home.startQuick", "Build a specialist agent")}
            </button>
          </div>
        </div>
        {hoveringQuick && (
          <p className="mt-3 text-xs text-cs-muted">
            {t("home.quickHint", "Persona on top of any runtime — @security-specialist on Gemini, @perf-reviewer on MiniMax, etc. Drop them into a war room session.")}
          </p>
        )}
      </section>

      {/* Alerts (only if present) */}
      {hasAlerts && (
        <section className="space-y-2">
          {alerts.map((a) => (
            <div
              key={a.id}
              className={`flex items-start gap-3 rounded-lg border p-4 ${
                a.severity === "danger"
                  ? "border-cs-danger/40 bg-cs-danger/10"
                  : "border-cs-warning/40 bg-cs-warning/10"
              }`}
            >
              <AlertCircle
                size={18}
                className={a.severity === "danger" ? "text-cs-danger" : "text-cs-warning"}
              />
              <span className="text-sm text-cs-text flex-1">{a.message}</span>
            </div>
          ))}
        </section>
      )}

      {/* Recent Agents */}
      <section>
        <header className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-medium text-cs-muted uppercase tracking-wide flex items-center gap-2">
            <Bot size={14} />
            {t("home.recentAgents", "Recent agents")}
          </h2>
          {hasAgents && (
            <button
              type="button"
              className="text-xs text-cs-muted hover:text-cs-text inline-flex items-center gap-1"
            >
              {t("home.seeAll", "See all")} <ArrowRight size={12} />
            </button>
          )}
        </header>

        {hasAgents ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            {effectiveRecentAgents.slice(0, 6).map((rec) => {
              const fullAgent = fetchedAgents?.find((a) => a.id === rec.id);
              return (
                <div
                  key={rec.id}
                  className="rounded-lg border border-cs-border bg-cs-card hover:border-cs-hover transition flex items-stretch overflow-hidden"
                >
                  <button
                    type="button"
                    onClick={() => onOpenAgent?.(rec.id)}
                    className="flex-1 text-left p-4 min-w-0"
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className={`inline-block w-2 h-2 rounded-full ${RUNTIME_DOT[rec.runtime]}`}
                      />
                      <span className="text-sm font-medium text-cs-text truncate">
                        {rec.displayName}
                      </span>
                    </div>
                    <div className="mt-1 text-xs text-cs-muted">{rec.runtime}</div>
                  </button>
                  {fullAgent && (() => {
                    const canRun = getRuntimeCapability(fullAgent.runtime).invocation.kind !== "manual";
                    return (
                      <button
                        type="button"
                        onClick={() => canRun && runInShell(fullAgent)}
                        disabled={!canRun}
                        className={
                          canRun
                            ? "px-3 border-l border-cs-border bg-cs-bg-raised text-cs-accent hover:bg-cs-accent/10 flex items-center gap-1 text-xs font-medium shrink-0"
                            : "px-3 border-l border-cs-border bg-cs-bg-raised text-cs-muted opacity-50 cursor-not-allowed flex items-center gap-1 text-xs font-medium shrink-0"
                        }
                        title={canRun ? "Open in Shell" : "Not yet supported from ATO for this runtime"}
                      >
                        <Play size={12} />
                      </button>
                    );
                  })()}
                </div>
              );
            })}
          </div>
        ) : (
          <EmptyCard
            icon={<Bot size={20} className="text-cs-muted" />}
            title={t("home.noAgentsTitle", "No agents yet")}
            body={t("home.noAgentsBody", "Create your first agent to see it here.")}
          />
        )}
      </section>

      {/* PR-C (2026-05-21) — Day-1 ROI scan tile. Mounted between Recent
          Agents and Recent Runs so a fresh user sees the value loop close
          (dispatch an agent → see savings + regressions) without scrolling.
          The tile defers its own data fetch via requestIdleCallback so it
          never delays first paint. */}
      <RoiScanTile onOpenInsights={onOpenInsights} />

      {/* Recent Runs */}
      <section>
        <header className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-medium text-cs-muted uppercase tracking-wide flex items-center gap-2">
            <Activity size={14} />
            {t("home.recentRuns", "Recent runs")}
          </h2>
          {hasRuns && (
            <button
              type="button"
              className="text-xs text-cs-muted hover:text-cs-text inline-flex items-center gap-1"
            >
              {t("home.seeAll", "See all")} <ArrowRight size={12} />
            </button>
          )}
        </header>

        {hasRuns ? (
          <ul className="rounded-lg border border-cs-border bg-cs-card divide-y divide-cs-border">
            {recentRuns.slice(0, 5).map((run) => (
              <li key={run.id}>
                <button
                  type="button"
                  onClick={() => onOpenRun?.(run.id)}
                  className="w-full flex items-center gap-3 px-4 py-3 text-left hover:bg-cs-bg-raised transition"
                >
                  <span className={`inline-block w-2 h-2 rounded-full ${STATUS_DOT[run.status]}`} />
                  <span className="text-sm text-cs-text flex-1 truncate">{run.agentName}</span>
                  <span className="text-xs text-cs-muted shrink-0">{run.runtime}</span>
                  <span className="text-xs text-cs-muted shrink-0 tabular-nums">
                    {formatRelative(run.startedAt)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <EmptyCard
            icon={<Activity size={20} className="text-cs-muted" />}
            title={t("home.noRunsTitle", "No runs yet")}
            body={t("home.noRunsBody", "Run an agent and the most recent executions show here.")}
          />
        )}
      </section>

      <CreateAgentWizard
        open={wizard !== null || uiCreateAgentOpen}
        initialPath={wizard ?? uiCreateAgentPath}
        onClose={() => {
          setWizard(null);
          closeUiCreateAgent();
        }}
      />

      {/* 2026-05-19 — FirstChatWizard moved to Dashboard.tsx so the
          bottom-pane "War room" launcher works from any section. The
          openFirstChat() / closeFirstChat actions still drive it via
          useUiStore; Home just stops mounting its own copy. */}

      {runningAgent && (
        <RunAgentDialog
          agent={runningAgent}
          open={!!runningAgent}
          onClose={() => setRunningAgent(null)}
        />
      )}
    </div>
  );
}

function EmptyCard({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 flex items-start gap-3">
      {icon}
      <div className="flex-1">
        <div className="text-sm text-cs-text">{title}</div>
        <div className="text-xs text-cs-muted mt-0.5">{body}</div>
      </div>
    </div>
  );
}

function agentToRecent(a: Agent): RecentAgent {
  return {
    id: a.id,
    displayName: a.displayName,
    runtime: a.runtime,
    lastUsedAt: a.lastUsedAt ? Date.parse(a.lastUsedAt) || null : null,
  };
}

function formatRelative(ts: number): string {
  const diff = Date.now() - ts;
  const m = Math.floor(diff / 60_000);
  if (m < 1) return "now";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.floor(h / 24);
  return `${d}d`;
}
