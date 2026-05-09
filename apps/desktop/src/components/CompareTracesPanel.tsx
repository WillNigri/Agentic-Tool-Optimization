import { lazy, Suspense, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  ArrowLeftRight,
  Cloud,
  Zap,
  Clock,
  CheckCircle2,
  XCircle,
} from "lucide-react";
import { getAgentTraces, type CloudAgentTrace } from "@/lib/cloudAgentTraces";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { cn } from "@/lib/utils";

// v2.0 stable — Compare sub-tab.
//
// External tab is for deployed bundles (kind=external) only. Pipelines
// is for multi-stage parent_run_id chains. Trace comparison is
// runtime-agnostic and kind-agnostic — it diffs any two cloud traces
// on the same agent — so it gets its own home rather than living
// behind the External-only filter (the previous home, where shipping
// the compare-demo required tagging an internal agent as external just
// to make it appear, which was dishonest).

const TraceCompareModal = lazy(() => import("./TraceCompareModal"));

interface AgentRow {
  agentSlug: string;
  runtime: string;
  kind: "internal" | "external";
  traces: CloudAgentTrace[];
  latestStartedAt: string;
  okRate: number;
}

export default function CompareTracesPanel() {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const mock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  const canQuery = mock || (isCloudUser && !!accessToken);
  const [openCompare, setOpenCompare] = useState<{ baselineTraceId: string; agentSlug: string } | null>(null);

  const tracesQuery = useQuery({
    queryKey: ["all-traces-for-compare"],
    queryFn: () => getAgentTraces(undefined, 500),
    enabled: canQuery && isPro,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });

  if (!isPro) {
    return (
      <Empty
        icon={<Zap size={20} />}
        title={t("insights.compare.proRequired", "Trace comparison is a Pro feature")}
        body={t(
          "insights.compare.proBody",
          "The eval workbench reads cloud-stored traces. Pro tier unlocks it.",
        )}
      />
    );
  }
  if (!canQuery) {
    return (
      <Empty
        icon={<Cloud size={20} />}
        title={t("insights.compare.signInRequired", "Sign in to compare traces")}
        body={t(
          "insights.compare.signInBody",
          "Comparing traces reads from ato-cloud — needs a cloud login. Settings → Cloud → Sign in.",
        )}
      />
    );
  }
  if (tracesQuery.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.compare.loading", "Loading traces…")}
      </div>
    );
  }
  if (tracesQuery.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("insights.compare.error", "Couldn't load traces")}: {String(tracesQuery.error)}
        </span>
      </div>
    );
  }

  // Group traces by agent_slug — only agents with ≥2 traces are
  // comparable. Single-trace agents have nothing to diff against.
  const allTraces = tracesQuery.data?.traces ?? [];
  const byAgent = new Map<string, CloudAgentTrace[]>();
  for (const tr of allTraces) {
    const list = byAgent.get(tr.agent_slug) ?? [];
    list.push(tr);
    byAgent.set(tr.agent_slug, list);
  }
  const rows: AgentRow[] = [];
  for (const [agentSlug, traces] of byAgent) {
    if (traces.length < 2) continue;
    traces.sort((a, b) => b.started_at.localeCompare(a.started_at));
    const okCount = traces.filter((t) => t.ok).length;
    rows.push({
      agentSlug,
      runtime: traces[0].runtime,
      kind: (traces[0] as { kind?: "internal" | "external" }).kind ?? "internal",
      traces,
      latestStartedAt: traces[0].started_at,
      okRate: okCount / traces.length,
    });
  }
  rows.sort((a, b) => b.latestStartedAt.localeCompare(a.latestStartedAt));

  return (
    <div className="space-y-4">
      <header>
        <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
          <ArrowLeftRight size={14} className="text-cs-accent" />
          {t("insights.compare.title", "Compare traces")}
        </h3>
        <p className="mt-0.5 text-[11px] text-cs-muted">
          {t(
            "insights.compare.subtitle",
            "Eval workbench. Pick an agent with ≥2 cloud traces, then diff any two to see duration / cost / file delta side-by-side. Works for any agent kind — internal CLI dispatches and deployed-bundle traces both land here.",
          )}
        </p>
      </header>

      {rows.length === 0 ? (
        <Empty
          icon={<ArrowLeftRight size={20} />}
          title={t("insights.compare.empty", "Nothing to compare yet")}
          body={t(
            "insights.compare.emptyBody",
            "Need at least 2 traces of the same agent. Fire two dispatches against any agent from the chat pane (same prompt or different — comparison works either way), then come back here.",
          )}
        />
      ) : (
        <ul className="space-y-2">
          {rows.map((row, i) => (
            <AgentCompareRow
              key={row.agentSlug}
              row={row}
              demoId={i === 0 ? "compare-agent-first" : undefined}
              onOpen={() =>
                setOpenCompare({
                  baselineTraceId: row.traces[0].id,
                  agentSlug: row.agentSlug,
                })
              }
            />
          ))}
        </ul>
      )}

      {openCompare && (
        <Suspense fallback={null}>
          <TraceCompareModal
            baselineTraceId={openCompare.baselineTraceId}
            agentSlug={openCompare.agentSlug}
            onClose={() => setOpenCompare(null)}
          />
        </Suspense>
      )}
    </div>
  );
}

function AgentCompareRow({
  row,
  onOpen,
  demoId,
}: {
  row: AgentRow;
  onOpen: () => void;
  demoId?: string;
}) {
  const { t } = useTranslation();
  return (
    <li>
      <button
        type="button"
        data-demo-id={demoId}
        onClick={onOpen}
        className={cn(
          "w-full text-left rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 transition-colors",
          "hover:border-cs-accent/40",
        )}
      >
        <div className="flex items-center gap-2">
          {row.okRate === 1 ? (
            <CheckCircle2 size={11} className="text-cs-accent shrink-0" />
          ) : (
            <XCircle size={11} className="text-cs-warn shrink-0" />
          )}
          <code className="font-mono text-sm text-cs-text font-medium">@{row.agentSlug}</code>
          <span className="text-[10px] uppercase tracking-wide text-cs-muted">{row.runtime}</span>
          <span className="text-[10px] uppercase tracking-wide text-cs-muted">{row.kind}</span>
          <span className="ml-auto inline-flex items-center gap-1 text-[10px] font-mono text-cs-muted">
            <Clock size={10} />
            {row.traces.length} {t("insights.compare.runs", "runs")}
          </span>
        </div>
        <div className="mt-1.5 text-[11px] text-cs-muted">
          {t("insights.compare.lastRun", "last run")}: {new Date(row.latestStartedAt).toLocaleString()}
          {row.okRate < 1 && (
            <span className="ml-2 text-cs-warn">
              · {Math.round((1 - row.okRate) * 100)}% {t("insights.compare.failed", "failed")}
            </span>
          )}
        </div>
        <div className="mt-1 text-[10px] text-cs-accent">
          {t("insights.compare.openCta", "Open compare workbench →")}
        </div>
      </button>
    </li>
  );
}

function Empty({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center text-sm">
      <div className="mx-auto mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-cs-accent/10 text-cs-accent">
        {icon}
      </div>
      <p className="text-cs-text font-medium mb-1">{title}</p>
      <p className="text-[12px] text-cs-muted">{body}</p>
    </div>
  );
}
