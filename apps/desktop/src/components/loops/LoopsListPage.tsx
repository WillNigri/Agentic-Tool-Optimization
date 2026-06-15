import { Fragment, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Clock3, History, Loader2, X } from "lucide-react";
import InitiatorBadge from "@/components/InitiatorBadge";
import ExecutionLogReceipt from "@/components/receipts/ExecutionLogReceipt";
import { cn } from "@/lib/utils";
import {
  get_loop_run_steps,
  list_loop_runs,
  list_loops,
  toggle_loop_enabled,
  type Loop,
  type LoopRun,
  type LoopRunStep,
} from "@/lib/loops-api";

interface LoopsListPageProps {
  onCreateLoop: () => void;
  onSelectLoop: (loop: Loop) => void;
}

const moneyFormatter = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
});

function formatTimestamp(value?: string | null): string {
  if (!value) return "Never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatDuration(durationMs?: number | null): string {
  if (durationMs == null) return "—";
  if (durationMs < 1000) return `${durationMs} ms`;
  const seconds = durationMs / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)} s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.round(seconds % 60);
  return `${minutes}m ${remainingSeconds}s`;
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function statusTone(status?: string | null): string {
  switch (status) {
    case "success":
      return "bg-emerald-500/10 text-emerald-300 border-emerald-500/20";
    case "error":
      return "bg-rose-500/10 text-rose-300 border-rose-500/20";
    case "running":
      return "bg-sky-500/10 text-sky-300 border-sky-500/20";
    case "paused":
      return "bg-amber-500/10 text-amber-300 border-amber-500/20";
    case "pending":
      return "bg-cs-accent/10 text-cs-accent border-cs-accent/20";
    default:
      return "bg-cs-bg-raised text-cs-muted border-cs-border";
  }
}

function StatusPill({ status, neverLabel = "Never" }: { status?: string | null; neverLabel?: string }) {
  const label = status ?? neverLabel;
  return (
    <span className={cn("inline-flex rounded-full border px-2 py-1 text-[11px] font-medium capitalize", statusTone(status))}>
      {label}
    </span>
  );
}

function RunStepsTable({ run }: { run: LoopRun }) {
  const { t } = useTranslation();
  const { data, isLoading, error } = useQuery({
    queryKey: ["loop-run-steps", run.id],
    queryFn: () => get_loop_run_steps(run.id),
  });

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 px-4 py-3 text-xs text-cs-muted">
        <Loader2 size={14} className="animate-spin" />
        {t("loops.runHistory.loadingSteps", "Loading step detail…")}
      </div>
    );
  }

  if (error) {
    const message = error instanceof Error ? error.message : String(error);
    return <div className="px-4 py-3 text-xs text-rose-300">{message}</div>;
  }

  if (!data || data.length === 0) {
    return <div className="px-4 py-3 text-xs text-cs-muted">{t("loops.runHistory.noSteps", "No steps recorded.")}</div>;
  }

  return (
    <div className="space-y-3 px-4 py-3">
      {data.map((step) => (
        <RunStepCard key={step.id} step={step} run={run} />
      ))}
    </div>
  );
}

function RunStepCard({ step, run }: { step: LoopRunStep; run: LoopRun }) {
  return (
    <div className="rounded-lg border border-cs-border bg-cs-bg-raised/60 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <StatusPill status={step.status} neverLabel="Unknown" />
        <span className="text-sm font-medium text-cs-text">{step.nodeId}</span>
        <span className="rounded-md bg-cs-card px-2 py-1 text-[11px] uppercase tracking-wide text-cs-muted">
          {step.nodeType}
        </span>
        <InitiatorBadge
          initiatorKind={run.initiatorKind}
          clientSurface={run.clientSurface}
          initiatorId={run.initiatorId}
        />
      </div>
      <div className="mt-2 grid gap-2 text-xs text-cs-muted md:grid-cols-3">
        <div>Started: {formatTimestamp(step.startedAt)}</div>
        <div>Finished: {formatTimestamp(step.finishedAt)}</div>
        <div className="flex items-center gap-1.5">
          <span>Execution log:</span>
          <ExecutionLogReceipt logId={step.executionLogId} />
        </div>
      </div>
      {step.error && (
        <div className="mt-3 rounded-md border border-rose-500/20 bg-rose-500/5 px-3 py-2 text-xs text-rose-200">
          {step.error}
        </div>
      )}
      {(step.input != null || step.output != null) && (
        <div className="mt-3 grid gap-3 lg:grid-cols-2">
          {step.input != null && (
            <div>
              <div className="mb-1 text-[11px] font-medium uppercase tracking-wide text-cs-muted">Input</div>
              <pre className="overflow-x-auto rounded-md border border-cs-border bg-cs-bg p-3 text-[11px] text-cs-text">
                {formatJson(step.input)}
              </pre>
            </div>
          )}
          {step.output != null && (
            <div>
              <div className="mb-1 text-[11px] font-medium uppercase tracking-wide text-cs-muted">Output</div>
              <pre className="overflow-x-auto rounded-md border border-cs-border bg-cs-bg p-3 text-[11px] text-cs-text">
                {formatJson(step.output)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function LoopHistoryDrawer({
  loop,
  onClose,
}: {
  loop: Loop;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [expandedRunId, setExpandedRunId] = useState<string | null>(null);
  const { data = [], isLoading, error } = useQuery({
    queryKey: ["loop-runs", loop.id],
    queryFn: () => list_loop_runs(loop.id, 25),
  });

  return (
    <div className="fixed inset-0 z-50 flex justify-end bg-black/50">
      <button type="button" aria-label={t("common.close", "Close")} className="flex-1 cursor-default" onClick={onClose} />
      <aside className="flex h-full w-full max-w-3xl flex-col border-l border-cs-border bg-cs-card shadow-2xl">
        <div className="flex items-start justify-between gap-4 border-b border-cs-border px-5 py-4">
          <div>
            <div className="text-sm font-semibold text-cs-text">{loop.name}</div>
            <div className="text-xs text-cs-muted">{loop.slug}</div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-cs-border p-2 text-cs-muted transition-colors hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-5">
          <div className="mb-4 flex items-center gap-2 text-sm font-medium text-cs-text">
            <History size={16} />
            {t("loops.runHistory.title", "Run history")}
          </div>

          {isLoading && (
            <div className="flex items-center gap-2 rounded-xl border border-cs-border bg-cs-bg-raised p-4 text-sm text-cs-muted">
              <Loader2 size={16} className="animate-spin" />
              {t("loops.runHistory.loading", "Loading recent runs…")}
            </div>
          )}

          {error && (
            <div className="rounded-xl border border-rose-500/20 bg-rose-500/5 p-4 text-sm text-rose-200">
              {error instanceof Error ? error.message : String(error)}
            </div>
          )}

          {!isLoading && !error && (
            <div className="overflow-hidden rounded-xl border border-cs-border bg-cs-bg-raised">
              <div className="overflow-x-auto">
                <table className="min-w-full text-left text-sm">
                  <thead className="border-b border-cs-border bg-cs-card/50 text-cs-muted">
                    <tr>
                      <th className="px-4 py-3 font-medium">Status</th>
                      <th className="px-4 py-3 font-medium">Started</th>
                      <th className="px-4 py-3 font-medium">Duration</th>
                      <th className="px-4 py-3 font-medium">Steps</th>
                      <th className="px-4 py-3 font-medium">Error</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.length === 0 && (
                      <tr>
                        <td colSpan={5} className="px-4 py-6 text-center text-cs-muted">
                          {t("loops.runHistory.empty", "No runs yet.")}
                        </td>
                      </tr>
                    )}
                    {data.map((run) => {
                      const isExpanded = expandedRunId === run.id;
                      return (
                        <Fragment key={run.id}>
                          <tr
                            className="cursor-pointer border-b border-cs-border/70 transition-colors hover:bg-cs-card/40"
                            onClick={() => setExpandedRunId(isExpanded ? null : run.id)}
                          >
                            <td className="px-4 py-3 align-top"><StatusPill status={run.status} neverLabel="Unknown" /></td>
                            <td className="px-4 py-3 align-top">{formatTimestamp(run.startedAt)}</td>
                            <td className="px-4 py-3 align-top">{formatDuration(run.durationMs)}</td>
                            <td className="px-4 py-3 align-top">{run.stepCount}</td>
                            <td className="max-w-xs px-4 py-3 align-top text-xs text-cs-muted">
                              {run.error ? <span className="line-clamp-2 text-rose-200">{run.error}</span> : "—"}
                            </td>
                          </tr>
                          {isExpanded && (
                            <tr className="border-b border-cs-border/70 bg-cs-card/30">
                              <td colSpan={5}>
                                <RunStepsTable run={run} />
                              </td>
                            </tr>
                          )}
                        </Fragment>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}

export default function LoopsListPage({ onCreateLoop, onSelectLoop }: LoopsListPageProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [historyLoop, setHistoryLoop] = useState<Loop | null>(null);
  const { data = [], isLoading, error } = useQuery({
    queryKey: ["loops"],
    queryFn: list_loops,
  });

  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => toggle_loop_enabled(id, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["loops"] });
    },
  });

  const totals = useMemo(() => {
    return data.reduce(
      (acc, loop) => {
        acc.dispatches += loop.dispatchCount;
        acc.cost += loop.totalCostUsd;
        return acc;
      },
      { dispatches: 0, cost: 0 },
    );
  }, [data]);

  return (
    <div className="flex h-full flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-cs-border bg-cs-card p-4">
        <div>
          <h2 className="text-lg font-semibold text-cs-text">{t("loops.list.title", "Loops")}</h2>
          <p className="text-sm text-cs-muted">
            {t("loops.list.subtitle", "Saved loop catalog with run status, dispatch volume, and cost receipts.")}
          </p>
        </div>
        <button
          type="button"
          onClick={onCreateLoop}
          className="rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg transition-colors hover:bg-cs-accent/90"
        >
          {t("loops.list.newLoop", "New loop")}
        </button>
      </div>

      <div className="grid gap-3 md:grid-cols-3">
        <div className="rounded-xl border border-cs-border bg-cs-card p-4">
          <div className="text-xs uppercase tracking-wide text-cs-muted">{t("loops.list.savedLoops", "Saved loops")}</div>
          <div className="mt-2 text-2xl font-semibold text-cs-text">{data.length}</div>
        </div>
        <div className="rounded-xl border border-cs-border bg-cs-card p-4">
          <div className="text-xs uppercase tracking-wide text-cs-muted">{t("loops.list.dispatchCount", "Dispatch count")}</div>
          <div className="mt-2 text-2xl font-semibold text-cs-text">{totals.dispatches}</div>
        </div>
        <div className="rounded-xl border border-cs-border bg-cs-card p-4">
          <div className="text-xs uppercase tracking-wide text-cs-muted">{t("loops.list.totalCost", "Total cost")}</div>
          <div className="mt-2 text-2xl font-semibold text-cs-text">{moneyFormatter.format(totals.cost)}</div>
        </div>
      </div>

      <div className="overflow-hidden rounded-xl border border-cs-border bg-cs-card">
        <div className="overflow-x-auto">
          <table className="min-w-full text-left text-sm">
            <thead className="border-b border-cs-border bg-cs-bg-raised/50 text-cs-muted">
              <tr>
                <th className="px-4 py-3 font-medium">Name</th>
                <th className="px-4 py-3 font-medium">Slug</th>
                <th className="px-4 py-3 font-medium">Kind</th>
                <th className="px-4 py-3 font-medium">Enabled</th>
                <th className="px-4 py-3 font-medium">Last run</th>
                <th className="px-4 py-3 font-medium">Last timestamp</th>
                <th className="px-4 py-3 font-medium">Dispatches</th>
                <th className="px-4 py-3 font-medium">Total cost</th>
                <th className="px-4 py-3 font-medium">History</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-cs-border">
              {isLoading && (
                <tr>
                  <td colSpan={9} className="px-4 py-8 text-center text-cs-muted">
                    <span className="inline-flex items-center gap-2">
                      <Loader2 size={16} className="animate-spin" />
                      {t("loops.list.loading", "Loading loops…")}
                    </span>
                  </td>
                </tr>
              )}
              {error && (
                <tr>
                  <td colSpan={9} className="px-4 py-8 text-center text-rose-200">
                    {error instanceof Error ? error.message : String(error)}
                  </td>
                </tr>
              )}
              {!isLoading && !error && data.length === 0 && (
                <tr>
                  <td colSpan={9} className="px-4 py-10 text-center">
                    <div className="mx-auto max-w-sm text-sm text-cs-muted">
                      <div className="mb-2 inline-flex rounded-full border border-cs-border p-3">
                        <Clock3 size={18} />
                      </div>
                      <div>{t("loops.list.empty", "No saved loops yet.")}</div>
                    </div>
                  </td>
                </tr>
              )}
              {!isLoading && !error && data.map((loop) => {
                const pendingToggle = toggleMutation.isPending && toggleMutation.variables?.id === loop.id;
                return (
                  <tr
                    key={loop.id}
                    className="cursor-pointer transition-colors hover:bg-cs-bg-raised/40"
                    onClick={() => onSelectLoop(loop)}
                  >
                    <td className="px-4 py-3">
                      <div className="font-medium text-cs-text">{loop.name}</div>
                      {loop.description && <div className="mt-1 line-clamp-2 text-xs text-cs-muted">{loop.description}</div>}
                    </td>
                    <td className="px-4 py-3 text-cs-muted">{loop.slug}</td>
                    <td className="px-4 py-3 capitalize text-cs-text">{loop.triggerKind}</td>
                    <td className="px-4 py-3">
                      <button
                        type="button"
                        aria-pressed={loop.enabled}
                        disabled={pendingToggle}
                        onClick={(event) => {
                          event.stopPropagation();
                          toggleMutation.mutate({ id: loop.id, enabled: !loop.enabled });
                        }}
                        className={cn(
                          "inline-flex h-6 w-11 items-center rounded-full border transition-colors",
                          loop.enabled ? "border-cs-accent/40 bg-cs-accent/20" : "border-cs-border bg-cs-bg-raised",
                          pendingToggle && "opacity-60",
                        )}
                      >
                        <span
                          className={cn(
                            "mx-0.5 h-4 w-4 rounded-full bg-white transition-transform",
                            loop.enabled ? "translate-x-5" : "translate-x-0",
                          )}
                        />
                      </button>
                    </td>
                    <td className="px-4 py-3"><StatusPill status={loop.lastRunStatus} /></td>
                    <td className="px-4 py-3 text-cs-muted">{formatTimestamp(loop.lastRunAt)}</td>
                    <td className="px-4 py-3 text-cs-text">{loop.dispatchCount}</td>
                    <td className="px-4 py-3 text-cs-text">{moneyFormatter.format(loop.totalCostUsd)}</td>
                    <td className="px-4 py-3">
                      <button
                        type="button"
                        onClick={(event) => {
                          event.stopPropagation();
                          setHistoryLoop(loop);
                        }}
                        className="inline-flex items-center gap-1 text-cs-accent transition-colors hover:text-cs-accent/80"
                      >
                        <History size={14} />
                        {t("loops.list.history", "History")}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>

      {historyLoop && <LoopHistoryDrawer loop={historyLoop} onClose={() => setHistoryLoop(null)} />}
    </div>
  );
}
