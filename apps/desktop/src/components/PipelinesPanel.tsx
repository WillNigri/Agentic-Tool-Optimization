import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  Sparkles,
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
import { PipelineModal } from "@/components/ExternalAgentsInsights";

// v2.0 stable — Pipelines sub-tab.
//
// External tab is for deployed bundles only (kind=external). But
// internal multi-agent dispatches (sequential groups like
// writer→reviewer, routed groups, anything with parent_run_id) also
// upload traces and belong somewhere visible — Beatriz hit the gap
// during pipe-demo verification when she fired a sequential group
// and got "No traces" on External tab.
//
// This panel reads cloud traces, groups by parent_run_id, shows one
// row per pipeline with stage count + total duration + chain summary
// (writer → reviewer arrows). Click any row → opens the existing
// PipelineModal with the full per-stage flow.

interface PipelineRow {
  parentRunId: string;
  stages: CloudAgentTrace[];
  startedAt: string;
  totalDurationMs: number;
  okCount: number;
  failCount: number;
}

export default function PipelinesPanel() {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const mock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  const canQuery = mock || (isCloudUser && accessToken);
  const [windowDays] = useState<7 | 30 | 90>(30);
  const [openPipeline, setOpenPipeline] = useState<string | null>(null);

  // Fetch enough recent traces to find pipelines. parent_run_id grouping
  // happens client-side because the cloud doesn't have a dedicated
  // "list pipelines" endpoint yet — getPipelineTraces fetches by ID,
  // not "all of them." 500 is the cloud's per-call cap.
  const tracesQuery = useQuery({
    queryKey: ["all-traces-for-pipelines", windowDays],
    queryFn: () => getAgentTraces(undefined, 500),
    enabled: !!canQuery && isPro,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });

  if (!isPro) {
    return (
      <Empty
        icon={<Zap size={20} />}
        title={t("insights.pipelines.proRequired", "Pipelines tracking is a Pro feature")}
        body={t(
          "insights.pipelines.proBody",
          "Multi-agent dispatches (sequential groups / routed groups) upload one trace per stage with a shared parent_run_id, then group together here. Pro tier unlocks it.",
        )}
      />
    );
  }
  if (!canQuery) {
    return (
      <Empty
        icon={<Cloud size={20} />}
        title={t("insights.pipelines.signInRequired", "Sign in to see pipelines")}
        body={t(
          "insights.pipelines.signInBody",
          "Pipeline traces live on ato-cloud — needs a cloud login. Settings → Cloud → Sign in.",
        )}
      />
    );
  }
  if (tracesQuery.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.pipelines.loading", "Loading pipelines…")}
      </div>
    );
  }
  if (tracesQuery.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("insights.pipelines.error", "Couldn't load pipelines")}: {String(tracesQuery.error)}
        </span>
      </div>
    );
  }

  // Group traces by parent_run_id. Skip traces without a parent
  // (those are single-agent dispatches; they live on External or
  // Insights → Agents).
  const allTraces = tracesQuery.data?.traces ?? [];
  const byParent = new Map<string, CloudAgentTrace[]>();
  for (const t of allTraces) {
    if (!t.parent_run_id) continue;
    const existing = byParent.get(t.parent_run_id) ?? [];
    existing.push(t);
    byParent.set(t.parent_run_id, existing);
  }

  const pipelines: PipelineRow[] = [];
  for (const [parentRunId, stages] of byParent) {
    stages.sort((a, b) => a.started_at.localeCompare(b.started_at));
    const startedAt = stages[0]?.started_at ?? new Date(0).toISOString();
    const totalDurationMs = stages.reduce((acc, s) => acc + (s.duration_ms ?? 0), 0);
    const okCount = stages.filter((s) => s.ok).length;
    const failCount = stages.length - okCount;
    pipelines.push({ parentRunId, stages, startedAt, totalDurationMs, okCount, failCount });
  }
  pipelines.sort((a, b) => b.startedAt.localeCompare(a.startedAt));

  return (
    <div className="space-y-4">
      <header>
        <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
          <Sparkles size={14} className="text-cs-accent" />
          {t("insights.pipelines.title", "Pipelines")}
        </h3>
        <p className="mt-0.5 text-[11px] text-cs-muted">
          {t(
            "insights.pipelines.subtitle",
            "Multi-stage dispatches grouped by parent_run_id. Sequential groups (writer → reviewer), routed groups, anything that fans out across runtimes. Click any row to see per-stage timing, files, and prompts.",
          )}
        </p>
      </header>

      {pipelines.length === 0 ? (
        <>
          <Empty
            icon={<Sparkles size={20} />}
            title={t("insights.pipelines.empty", "No pipelines in this window")}
            body={t(
              "insights.pipelines.emptyBody",
              "Fire a sequential group (writer → reviewer) or a routed group from the chat pane, then come back here. Each stage uploads its own trace; we group them by parent_run_id.",
            )}
          />
          {/* Diagnostic readout — distinguishes "upload broken" (0 traces)
              from "parent_run_id missing" (traces present, none grouped).
              Shown only when there are zero pipelines so it doesn't add
              noise to the success path. */}
          <div className="mt-3 rounded-md border border-cs-border/60 bg-cs-bg-raised/30 px-3 py-2 font-mono text-[10px] text-cs-muted">
            <div>diagnostic — fetched {allTraces.length} total traces from cloud</div>
            <div>
              with parent_run_id: {allTraces.filter((t) => t.parent_run_id).length}
              {" · "}without: {allTraces.filter((t) => !t.parent_run_id).length}
            </div>
            {allTraces.length === 0 && (
              <div className="mt-1 text-cs-warn">
                no traces at all → upload is failing. Check DevTools Network for POST /api/agent-traces, or the desktop's tier gate (tierMeetsRequirement). Auth-store tokens may not be attaching.
              </div>
            )}
            {allTraces.length > 0 && allTraces.filter((t) => t.parent_run_id).length === 0 && (
              <div className="mt-1 text-cs-warn">
                traces uploaded but none have parent_run_id → the dispatch path isn't tagging stages. Check PromptBar.tsx group-dispatch upload loop.
              </div>
            )}
          </div>
        </>
      ) : (
        <ul className="space-y-2">
          {pipelines.map((p, i) => (
            <PipelineRowCard
              key={p.parentRunId}
              row={p}
              onClick={setOpenPipeline}
              demoId={i === 0 ? "pipeline-row-first" : undefined}
            />
          ))}
        </ul>
      )}
      {openPipeline && (
        <PipelineModal
          parentRunId={openPipeline}
          onClose={() => setOpenPipeline(null)}
        />
      )}
    </div>
  );
}

function PipelineRowCard({
  row,
  onClick,
  demoId,
}: {
  row: PipelineRow;
  onClick?: (parentRunId: string) => void;
  demoId?: string;
}) {
  const { t } = useTranslation();
  const chainLabel = row.stages
    .map((s) => `@${s.agent_slug} (${s.runtime})`)
    .join(" → ");
  const groupSlug =
    row.stages[0]?.metadata && typeof (row.stages[0].metadata as { groupSlug?: unknown }).groupSlug === "string"
      ? (row.stages[0].metadata as { groupSlug: string }).groupSlug
      : null;
  return (
    <li>
      <button
        type="button"
        data-demo-id={demoId}
        onClick={() => onClick?.(row.parentRunId)}
        className={cn(
          "w-full text-left rounded-lg border p-3 transition-colors",
          row.failCount > 0
            ? "border-cs-warn/40 bg-cs-warn/5 hover:border-cs-warn"
            : "border-cs-border bg-cs-bg-raised/40 hover:border-cs-accent/40",
        )}
      >
        <div className="flex items-center gap-2">
          {row.failCount === 0 ? (
            <CheckCircle2 size={11} className="text-cs-accent shrink-0" />
          ) : (
            <XCircle size={11} className="text-cs-warn shrink-0" />
          )}
          {groupSlug && (
            <code className="font-mono text-sm text-cs-text font-medium">@{groupSlug}</code>
          )}
          <span className="text-[10px] uppercase tracking-wide text-cs-muted">
            {row.stages.length} {row.stages.length === 1 ? t("insights.pipelines.stage", "stage") : t("insights.pipelines.stages", "stages")}
          </span>
          <span className="ml-auto inline-flex items-center gap-1 text-[10px] font-mono text-cs-muted">
            <Clock size={10} />
            {row.totalDurationMs > 1000
              ? `${(row.totalDurationMs / 1000).toFixed(1)}s`
              : `${row.totalDurationMs}ms`}
          </span>
        </div>
        <div className="mt-1.5 font-mono text-[11px] text-cs-muted truncate">
          {chainLabel}
        </div>
        <div className="mt-1 text-[10px] text-cs-muted">
          {new Date(row.startedAt).toLocaleString()}
          {row.failCount > 0 && (
            <span className="ml-2 text-cs-warn">
              · {row.failCount} {row.failCount === 1 ? "stage failed" : "stages failed"}
            </span>
          )}
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
