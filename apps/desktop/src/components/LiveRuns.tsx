import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Loader2,
  Activity,
  Square,
  AlertCircle,
  Folder,
  Cpu,
  Clock,
  Sparkles,
  StopCircle,
  Eye,
} from "lucide-react";
import {
  listActiveRuns,
  killActiveRun,
  billingSurfaceShortLabel,
  billingSurfaceLabel,
  type ActiveRun,
} from "@/lib/activeRuns";
import { cn } from "@/lib/utils";

// v2.6 PR-A — three-way filter so the user can isolate ATO's own
// dispatches from foreign-CLI sessions the passive observer surfaces.
type SourceFilter = "all" | "ato" | "observed";

// v2.1.0 Phase 4 — Live runs panel.
//
// The "missing ops layer" Timur Yessenov asked for on Twitter: which
// runtime is in which workspace, what's running right now, and a
// kill button per row so you don't have to read every terminal
// buffer to stop a stuck dispatch.
//
// Data lives in the Rust active_runs registry (process-wide
// Mutex<HashMap>). Polled every 2s — frequent enough for the panel
// to feel live, infrequent enough not to hammer the IPC bridge.

export default function LiveRuns() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [killing, setKilling] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<SourceFilter>("all");

  const query = useQuery({
    queryKey: ["active-runs"],
    queryFn: listActiveRuns,
    refetchInterval: 2_000,
    staleTime: 0,
  });

  const onKill = async (runId: string) => {
    setKilling((prev) => new Set(prev).add(runId));
    try {
      await killActiveRun(runId);
      // Force refresh so the row updates to status='killing' or
      // disappears entirely.
      await queryClient.invalidateQueries({ queryKey: ["active-runs"] });
    } finally {
      setKilling((prev) => {
        const next = new Set(prev);
        next.delete(runId);
        return next;
      });
    }
  };

  // v2.1.8+ — bulk kill. Confirm before firing N kills since this is
  // destructive (each run gets SIGKILL'd, in-flight responses lost).
  // Fires sequentially with Promise.allSettled so a single hang
  // doesn't block the rest.
  const onKillAll = async () => {
    // v2.6 PR-A — only ATO's own dispatches are killable. Passive
    // observations (external CLI sessions) aren't processes we own,
    // so they're excluded from the bulk-kill set even when the
    // "All" filter is active.
    const killable = (query.data ?? []).filter(
      (r) => r.dispatch_kind !== "passive_observation",
    );
    if (killable.length === 0) return;
    const confirmed = window.confirm(
      t(
        "insights.live.confirmKillAll",
        "Kill all {{n}} running dispatches? In-flight responses will be lost.",
        { n: killable.length },
      ),
    );
    if (!confirmed) return;
    setKilling(new Set(killable.map((r) => r.run_id)));
    try {
      await Promise.allSettled(killable.map((r) => killActiveRun(r.run_id)));
      await queryClient.invalidateQueries({ queryKey: ["active-runs"] });
    } finally {
      setKilling(new Set());
    }
  };

  if (query.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.live.loading", "Loading active runs…")}
      </div>
    );
  }

  if (query.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("insights.live.error", "Couldn't load active runs")}: {String(query.error)}
        </span>
      </div>
    );
  }

  const allRuns = query.data ?? [];
  // v2.6 PR-A — split + counts so the filter pills always render with
  // an honest tally even when one bucket is empty.
  const atoRuns = useMemo(
    () => allRuns.filter((r) => r.dispatch_kind !== "passive_observation"),
    [allRuns],
  );
  const observedRuns = useMemo(
    () => allRuns.filter((r) => r.dispatch_kind === "passive_observation"),
    [allRuns],
  );
  const runs =
    filter === "ato" ? atoRuns : filter === "observed" ? observedRuns : allRuns;

  return (
    <div className="space-y-3">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <Activity size={14} className="text-cs-accent" />
            {t("insights.live.title", "Live runs")}
            {allRuns.length > 0 && (
              <span className="ml-1 inline-flex h-1.5 w-1.5 animate-pulse rounded-full bg-cs-accent" />
            )}
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.live.subtitle",
              "Every dispatch in flight right now — ATO's and any external CLI session we observe. Kill stuck runs without reading the terminal buffer.",
            )}
          </p>
        </div>
        {/* v2.1.8 — bulk kill. Only kills ATO's own runs — passive
            observations aren't processes we can signal. Confirmation
            dialog avoids accidental nuke during demos. */}
        {atoRuns.length >= 2 && (
          <button
            type="button"
            onClick={onKillAll}
            disabled={killing.size > 0}
            className="shrink-0 inline-flex items-center gap-1.5 rounded-md border border-cs-warn/40 bg-cs-warn/10 px-2 py-1 text-[11px] font-medium text-cs-warn hover:bg-cs-warn/20 disabled:opacity-60"
          >
            <StopCircle size={11} />
            {t("insights.live.killAll", "Kill all ({{n}})", { n: atoRuns.length })}
          </button>
        )}
      </header>

      {/* v2.6 PR-A — source filter. Always rendered when any observed
          rows exist so the user knows external sessions are being
          surfaced; hidden entirely on installs with no observed rows
          ever to avoid cluttering the panel for solo users. */}
      {(observedRuns.length > 0 || filter !== "all") && (
        <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5 text-[11px]">
          {(
            [
              ["all", t("insights.live.filterAll", "All"), allRuns.length],
              ["ato", t("insights.live.filterAto", "ATO dispatches"), atoRuns.length],
              [
                "observed",
                t("insights.live.filterObserved", "Observed sessions"),
                observedRuns.length,
              ],
            ] as const
          ).map(([key, label, count]) => (
            <button
              key={key}
              type="button"
              onClick={() => setFilter(key)}
              className={cn(
                "rounded px-2.5 py-1 font-medium transition",
                filter === key
                  ? "bg-cs-accent/15 text-cs-accent"
                  : "text-cs-muted hover:text-cs-text",
              )}
            >
              {label}
              <span className="ml-1 text-cs-muted/80">({count})</span>
            </button>
          ))}
        </div>
      )}

      {runs.length === 0 ? (
        <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center text-sm">
          <div className="mx-auto mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-cs-bg-raised text-cs-muted">
            <Activity size={20} />
          </div>
          <p className="text-cs-text font-medium mb-1">
            {t("insights.live.idle", "No agents running")}
          </p>
          <p className="text-[12px] text-cs-muted">
            {t(
              "insights.live.idleBody",
              "When you dispatch an agent (Quick test, chat pane, MCP run_agent, scheduled cron), it'll appear here with a kill button.",
            )}
          </p>
        </div>
      ) : (
        <ul className="space-y-1.5">
          {runs.map((run) => (
            <RunRow
              key={run.run_id}
              run={run}
              killing={killing.has(run.run_id)}
              onKill={() => onKill(run.run_id)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function RunRow({
  run,
  killing,
  onKill,
}: {
  run: ActiveRun;
  killing: boolean;
  onKill: () => void;
}) {
  const { t } = useTranslation();
  const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - run.started_at_unix);
  const elapsedLabel =
    elapsed < 60
      ? `${elapsed}s`
      : elapsed < 3600
        ? `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`
        : `${Math.floor(elapsed / 3600)}h ${Math.floor((elapsed % 3600) / 60)}m`;
  const isKilling = run.status === "killing" || killing;
  const workspaceShort = run.workspace
    ? run.workspace.split("/").filter(Boolean).slice(-2).join("/")
    : null;
  const isPassive = run.dispatch_kind === "passive_observation";
  const surfaceShort = billingSurfaceShortLabel(run.billing_surface);
  const surfaceFull = billingSurfaceLabel(run.billing_surface);

  return (
    <li
      className={cn(
        "flex items-center gap-3 rounded-lg border p-3 transition-colors",
        isKilling
          ? "border-cs-warn/40 bg-cs-warn/10"
          : isPassive
            ? // Visually distinct so observed rows don't read as ATO
              // dispatches — softer border, no hover accent.
              "border-cs-border/60 bg-cs-bg-raised/20"
            : "border-cs-border bg-cs-bg-raised/40 hover:border-cs-accent/40",
      )}
    >
      <span className="relative inline-flex shrink-0">
        {isPassive ? (
          // Eye marks "we're watching, not dispatching."
          <Eye size={14} className="text-cs-muted" />
        ) : (
          <Sparkles size={14} className="text-cs-accent" />
        )}
        {!isKilling && !isPassive && (
          <span className="absolute -right-1 -top-1 inline-flex h-2 w-2 rounded-full bg-cs-accent">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-cs-accent opacity-60" />
          </span>
        )}
      </span>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-xs flex-wrap">
          {run.agent_slug ? (
            <code className="font-mono text-cs-text font-medium">@{run.agent_slug}</code>
          ) : (
            <span className="text-cs-muted italic">
              {isPassive
                ? t("insights.live.observed", "observed session")
                : t("insights.live.adhoc", "ad-hoc")}
            </span>
          )}
          <span className="inline-flex items-center gap-1 text-[10px] uppercase tracking-wide text-cs-muted">
            <Cpu size={10} />
            {run.runtime}
          </span>
          {/* v2.6 PR-A — billing surface chip. Hidden when unknown
              (openclaw/hermes runtimes that don't expose an auth-mode
              signal) to avoid an "Unknown" badge polluting the row. */}
          {run.billing_surface && (
            <span
              className={cn(
                "inline-flex items-center rounded-full border px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide",
                isPassive
                  ? "border-cs-border bg-cs-bg-raised text-cs-muted"
                  : "border-cs-accent/30 bg-cs-accent/10 text-cs-accent",
              )}
              title={surfaceFull}
            >
              {surfaceShort}
            </span>
          )}
          <span className="inline-flex items-center gap-1 text-[10px] font-mono text-cs-muted">
            <Clock size={10} />
            {elapsedLabel}
          </span>
        </div>
        {workspaceShort && (
          <div className="mt-0.5 inline-flex items-center gap-1 text-[11px] text-cs-muted truncate">
            <Folder size={10} />
            <code className="font-mono truncate" title={run.workspace ?? undefined}>
              {workspaceShort}
            </code>
          </div>
        )}
        {run.source && !isPassive && (
          <div className="mt-0.5 text-[10px] text-cs-muted">{run.source}</div>
        )}
      </div>

      {/* Passive rows aren't ATO processes; we can't signal them.
          Render a non-interactive "watching" pill instead of a kill
          button so the row doesn't look broken. */}
      {isPassive ? (
        <span
          className="shrink-0 inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-2.5 py-1.5 text-[11px] text-cs-muted"
          title={t(
            "insights.live.watchingTitle",
            "ATO is observing this external CLI session via on-disk logs. Not killable from here.",
          )}
        >
          <Eye size={11} />
          {t("insights.live.watching", "watching")}
        </span>
      ) : (
        <button
          type="button"
          onClick={onKill}
          disabled={isKilling}
          className={cn(
            "inline-flex items-center gap-1 rounded-md border px-3 py-1.5 text-[11px] font-medium transition-colors shrink-0",
            isKilling
              ? "border-cs-warn/40 bg-cs-warn/10 text-cs-warn"
              : "border-cs-danger/40 bg-cs-danger/5 text-cs-danger hover:bg-cs-danger/15",
          )}
        >
          {isKilling ? (
            <>
              <Loader2 size={11} className="animate-spin" />
              {t("insights.live.killing", "Killing…")}
            </>
          ) : (
            <>
              <Square size={11} />
              {t("insights.live.kill", "Kill")}
            </>
          )}
        </button>
      )}
    </li>
  );
}
