// v2.10 PR-8 — Methodologies tab.
//
// Read-only view of methodologies + runs from the local SQLite DB.
// Free tier: full visibility into your own dispatches you already paid
// for. Pro tier: cloud-backed comparisons + scheduled runs land
// progressively as separate features (gated via `methodology.cloud`).
//
// What we surface here (free):
//   - List of every methodology defined (`ato evaluations methodology create`)
//   - Per-methodology run history (newest first)
//   - Per-run composition: variant cells, mean cost, mean score, pass
//     rate, with 95% confidence intervals via the same Student's t
//     table the CLI uses.
//
// What we do NOT surface here (the "private info" constraint):
//   - Admin margin queries (the CLI `methodology margin` command is
//     the audit surface; the UI doesn't show our cost ledger because
//     it's strictly local — every customer's local margin is theirs
//     anyway, but baking the admin SQL into the OSS UI invites
//     confusion).
//   - Cloud endpoints (no fetch to ato-cloud).
//   - Internal calibration constants (those live in pricing.json,
//     open-source already).
//
// All numbers in this panel come from local SQLite via three Tauri
// commands (list_methodology_definitions / list_methodology_runs /
// get_methodology_run_detail). No background syncing, no auto-refresh
// beyond the React Query stale window.

import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  Beaker,
  Loader2,
  RotateCw,
  ArrowLeft,
  ArrowRight,
  AlertCircle,
  CheckCircle2,
  TrendingUp,
  TrendingDown,
  Clock,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { composeCells, type CellSummary } from "./compose";

interface MethodologyView {
  id: string;
  slug: string;
  description: string | null;
  archetype: string;
  variant_matrix: string;
  rubric: string;
  created_at: string;
  run_count: number;
}

interface MethodologyRunView {
  id: string;
  methodology_slug: string;
  methodology_archetype: string;
  started_at: string;
  ended_at: string | null;
  status: string;
  planned: number;
  completed: number;
  customer_cost_usd: number;
  customer_tokens_in: number;
  customer_tokens_out: number;
  provider_total_cost_usd: number;
  provider_judge_cost_usd: number;
  margin_usd: number;
  billing_mode: string;
}

interface MethodologyDispatchView {
  execution_log_id: string;
  variant_cell: string;
  score: number | null;
  cost_usd: number | null;
  tokens_in: number | null;
  tokens_out: number | null;
  duration_ms: number | null;
  status: string | null;
  grounding_verdict: string | null;
  runtime: string | null;
  model: string | null;
  created_at: string | null;
}

interface MethodologyRunDetail {
  run: MethodologyRunView;
  dispatches: MethodologyDispatchView[];
}

type View =
  | { kind: "list" }
  | { kind: "methodology"; slug: string }
  | { kind: "run"; runId: string };

export default function MethodologiesPanel() {
  const { t } = useTranslation();
  const [view, setView] = useState<View>({ kind: "list" });

  const methodologies = useQuery({
    queryKey: ["methodology", "definitions"],
    queryFn: () =>
      invoke<MethodologyView[]>("list_methodology_definitions"),
    staleTime: 30_000,
  });

  if (methodologies.isLoading) {
    return (
      <div className="flex items-center justify-center p-12 text-cs-muted">
        <Loader2 className="w-5 h-5 animate-spin mr-2" />
        {t("methodology.loading", "Loading methodologies…")}
      </div>
    );
  }

  if (methodologies.isError) {
    return (
      <div className="m-6 p-4 border border-red-500/30 bg-red-500/10 rounded-lg text-sm">
        <div className="flex items-center gap-2 text-red-400 font-medium mb-1">
          <AlertCircle className="w-4 h-4" />
          {t("methodology.error", "Failed to load methodologies")}
        </div>
        <code className="text-xs text-red-300">
          {String(methodologies.error)}
        </code>
      </div>
    );
  }

  if (view.kind === "run") {
    return (
      <RunDetailView
        runId={view.runId}
        onBack={() => setView({ kind: "list" })}
      />
    );
  }

  if (view.kind === "methodology") {
    return (
      <MethodologyRunsView
        slug={view.slug}
        onBack={() => setView({ kind: "list" })}
        onOpenRun={(runId) => setView({ kind: "run", runId })}
      />
    );
  }

  return (
    <MethodologyListView
      methodologies={methodologies.data ?? []}
      onOpen={(slug) => setView({ kind: "methodology", slug })}
      onOpenRun={(runId) => setView({ kind: "run", runId })}
      onRefresh={() => methodologies.refetch()}
    />
  );
}

// ── List view: every methodology + the latest 5 runs across all ──────────

function MethodologyListView({
  methodologies,
  onOpen,
  onOpenRun,
  onRefresh,
}: {
  methodologies: MethodologyView[];
  onOpen: (slug: string) => void;
  onOpenRun: (runId: string) => void;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const recentRuns = useQuery({
    queryKey: ["methodology", "runs", "recent"],
    queryFn: () =>
      invoke<MethodologyRunView[]>("list_methodology_runs", { limit: 10 }),
    staleTime: 30_000,
  });

  return (
    <div className="p-6 space-y-6">
      <header className="flex items-center justify-between">
        <div>
          <div className="flex items-center gap-2 text-cs-accent text-xs uppercase tracking-wide font-semibold">
            <Beaker className="w-4 h-4" />
            {t("methodology.headerLabel", "Methodology Runner")}
          </div>
          <h2 className="text-cs-fg text-xl font-semibold mt-1">
            {t(
              "methodology.headerTitle",
              "Methodologies — your evals, your receipts",
            )}
          </h2>
          <p className="text-cs-muted text-sm mt-2 max-w-2xl">
            {t(
              "methodology.headerBody",
              "Reusable test recipes. Each run scores N×M×R dispatches against a rubric and writes a dual-cost-accounting receipt. Open one to see the per-cell composition.",
            )}
          </p>
        </div>
        <button
          onClick={onRefresh}
          className="text-cs-muted hover:text-cs-fg transition-colors"
          title={t("methodology.refresh", "Refresh")}
        >
          <RotateCw className="w-4 h-4" />
        </button>
      </header>

      {methodologies.length === 0 ? (
        <EmptyState />
      ) : (
        <section>
          <h3 className="text-cs-fg font-medium text-sm mb-3">
            {t("methodology.definitions", "Methodologies")} ({methodologies.length})
          </h3>
          <div className="grid gap-3">
            {methodologies.map((m) => (
              <MethodologyCard
                key={m.id}
                methodology={m}
                onOpen={() => onOpen(m.slug)}
              />
            ))}
          </div>
        </section>
      )}

      {recentRuns.data && recentRuns.data.length > 0 && (
        <section>
          <h3 className="text-cs-fg font-medium text-sm mb-3">
            {t("methodology.recentRuns", "Recent runs")}
          </h3>
          <div className="space-y-2">
            {recentRuns.data.slice(0, 10).map((r) => (
              <RunRow key={r.id} run={r} onOpen={() => onOpenRun(r.id)} />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="border border-cs-border/40 rounded-lg p-8 text-center">
      <Beaker className="w-8 h-8 text-cs-accent mx-auto mb-3 opacity-60" />
      <h3 className="text-cs-fg font-medium mb-2">
        {t("methodology.emptyTitle", "No methodologies yet")}
      </h3>
      <p className="text-cs-muted text-sm max-w-md mx-auto">
        {t(
          "methodology.emptyBody",
          "A methodology is a reusable test recipe — N prompts × M models × R repetitions, scored through a rubric. Create one from the CLI to populate this view.",
        )}
      </p>
      <pre className="text-xs text-cs-muted bg-cs-card/40 border border-cs-border/30 rounded p-3 mt-4 text-left max-w-md mx-auto overflow-x-auto">
        <code>{`ato evaluations methodology create \\
  --config my-methodology.json`}</code>
      </pre>
      <p className="text-cs-muted text-xs mt-3">
        {t(
          "methodology.emptyDocs",
          "See docs/methodology-runner.md in the repo for the config shape.",
        )}
      </p>
    </div>
  );
}

function MethodologyCard({
  methodology: m,
  onOpen,
}: {
  methodology: MethodologyView;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  const matrix = useMemo(() => {
    try {
      return JSON.parse(m.variant_matrix) as {
        prompts?: unknown[];
        models?: unknown[];
        conditions?: unknown[];
        reps_per_cell?: number;
        runtime?: string;
      };
    } catch {
      return null;
    }
  }, [m.variant_matrix]);
  const rubric = useMemo(() => {
    try {
      return JSON.parse(m.rubric) as { kind?: string };
    } catch {
      return null;
    }
  }, [m.rubric]);
  const dispatchesPerRun = matrix
    ? Math.max(1, matrix.prompts?.length ?? 1) *
      Math.max(1, matrix.models?.length ?? 1) *
      Math.max(1, matrix.conditions?.length ?? 1) *
      Math.max(1, matrix.reps_per_cell ?? 1)
    : 0;

  return (
    <button
      onClick={onOpen}
      className="text-left w-full border border-cs-border/40 hover:border-cs-accent/50 rounded-lg p-4 transition-colors bg-cs-card/30"
    >
      <div className="flex items-center justify-between gap-3 mb-2">
        <div className="flex items-center gap-2 min-w-0">
          <code className="text-cs-accent font-medium text-sm truncate">
            {m.slug}
          </code>
          <span className="text-[10px] uppercase tracking-wide text-cs-muted bg-cs-card/60 px-1.5 py-0.5 rounded">
            {m.archetype}
          </span>
          {rubric?.kind && (
            <span className="text-[10px] uppercase tracking-wide text-cs-accent/80 border border-cs-accent/30 px-1.5 py-0.5 rounded">
              {rubric.kind}
            </span>
          )}
        </div>
        <ArrowRight className="w-4 h-4 text-cs-muted shrink-0" />
      </div>
      {m.description && (
        <p className="text-cs-fg/70 text-sm mb-2">{m.description}</p>
      )}
      <div className="text-cs-muted text-xs flex items-center gap-4 flex-wrap">
        <span>
          {dispatchesPerRun}{" "}
          {t("methodology.dispatchesPerRun", "dispatches per run")}
        </span>
        <span>·</span>
        <span>
          {m.run_count} {t("methodology.runs", "runs")}
        </span>
        <span>·</span>
        <span>
          {t("methodology.created", "Created")}{" "}
          {new Date(m.created_at).toLocaleDateString()}
        </span>
      </div>
    </button>
  );
}

// ── Per-methodology runs list ───────────────────────────────────────────

function MethodologyRunsView({
  slug,
  onBack,
  onOpenRun,
}: {
  slug: string;
  onBack: () => void;
  onOpenRun: (runId: string) => void;
}) {
  const { t } = useTranslation();
  const runs = useQuery({
    queryKey: ["methodology", "runs", slug],
    queryFn: () =>
      invoke<MethodologyRunView[]>("list_methodology_runs", {
        methodologySlug: slug,
        limit: 200,
      }),
    staleTime: 30_000,
  });

  return (
    <div className="p-6 space-y-4">
      <button
        onClick={onBack}
        className="text-cs-muted hover:text-cs-fg text-sm flex items-center gap-1"
      >
        <ArrowLeft className="w-4 h-4" />
        {t("methodology.backToList", "Back to methodologies")}
      </button>
      <h2 className="text-cs-fg text-xl font-semibold">
        <code className="text-cs-accent">{slug}</code>
      </h2>
      {runs.isLoading && (
        <div className="text-cs-muted text-sm flex items-center gap-2">
          <Loader2 className="w-4 h-4 animate-spin" />
          {t("methodology.loadingRuns", "Loading runs…")}
        </div>
      )}
      {runs.data && runs.data.length === 0 && (
        <div className="text-cs-muted text-sm">
          {t(
            "methodology.noRunsYet",
            "No runs yet for this methodology. Use `ato evaluations methodology run` or `adopt` to populate it.",
          )}
        </div>
      )}
      {runs.data && runs.data.length > 0 && (
        <div className="space-y-2">
          {runs.data.map((r) => (
            <RunRow key={r.id} run={r} onOpen={() => onOpenRun(r.id)} />
          ))}
        </div>
      )}
    </div>
  );
}

function RunRow({
  run,
  onOpen,
}: {
  run: MethodologyRunView;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  const statusColor =
    run.status === "complete"
      ? "text-green-400 border-green-500/30 bg-green-500/10"
      : run.status === "failed"
      ? "text-red-400 border-red-500/30 bg-red-500/10"
      : "text-cs-muted border-cs-border/40 bg-cs-card/40";
  return (
    <button
      onClick={onOpen}
      className="w-full text-left border border-cs-border/30 hover:border-cs-accent/40 rounded-lg p-3 transition-colors flex items-center gap-3"
    >
      <span
        className={cn(
          "text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border font-semibold",
          statusColor,
        )}
      >
        {run.status}
      </span>
      <code className="text-cs-accent text-sm">{run.methodology_slug}</code>
      <span className="text-cs-muted text-xs">
        {run.completed}/{run.planned}{" "}
        {t("methodology.dispatches", "dispatches")}
      </span>
      <span className="text-cs-muted text-xs ml-auto flex items-center gap-1">
        <Clock className="w-3 h-3" />
        {new Date(run.started_at).toLocaleString()}
      </span>
      <span className="text-cs-fg/80 text-xs font-mono">
        ${run.customer_cost_usd.toFixed(4)}
      </span>
      <ArrowRight className="w-4 h-4 text-cs-muted shrink-0" />
    </button>
  );
}

// ── Run detail with composition ─────────────────────────────────────────

function RunDetailView({
  runId,
  onBack,
}: {
  runId: string;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const detail = useQuery({
    queryKey: ["methodology", "run", runId],
    queryFn: () =>
      invoke<MethodologyRunDetail>("get_methodology_run_detail", { runId }),
    staleTime: 30_000,
  });

  if (detail.isLoading) {
    return (
      <div className="p-6 text-cs-muted flex items-center gap-2">
        <Loader2 className="w-4 h-4 animate-spin" />
        {t("methodology.loadingRun", "Loading run detail…")}
      </div>
    );
  }
  if (detail.isError || !detail.data) {
    return (
      <div className="p-6">
        <button
          onClick={onBack}
          className="text-cs-muted hover:text-cs-fg text-sm flex items-center gap-1 mb-4"
        >
          <ArrowLeft className="w-4 h-4" />
          {t("methodology.backToList", "Back")}
        </button>
        <div className="text-red-400 text-sm">
          {t("methodology.runNotFound", "Run not found")}
        </div>
      </div>
    );
  }

  const { run, dispatches } = detail.data;
  const cells: CellSummary[] = composeCells(dispatches);
  // Code-review finding #1 (claude): falsy-0 mistaken for a real zero
  // score. We must return null when no cell has a rubric score, not 0.
  const scoredCells = cells.filter((c) => c.score !== null);
  const overallMeanScore =
    scoredCells.length > 0
      ? scoredCells.reduce((acc, c) => acc + (c.score?.mean ?? 0), 0) /
        scoredCells.length
      : null;

  return (
    <div className="p-6 space-y-5">
      <button
        onClick={onBack}
        className="text-cs-muted hover:text-cs-fg text-sm flex items-center gap-1"
      >
        <ArrowLeft className="w-4 h-4" />
        {t("methodology.backToList", "Back")}
      </button>

      <header>
        <div className="flex items-center gap-2 text-cs-accent text-xs uppercase tracking-wide font-semibold">
          <Beaker className="w-4 h-4" />
          {t("methodology.runHeader", "Methodology run")}
        </div>
        <h2 className="text-cs-fg text-xl font-semibold mt-1">
          <code className="text-cs-accent">{run.methodology_slug}</code>{" "}
          <span className="text-cs-muted text-sm font-normal">[{run.id.slice(0, 8)}…]</span>
        </h2>
        <p className="text-cs-muted text-xs mt-1">
          {new Date(run.started_at).toLocaleString()}
          {run.ended_at && ` → ${new Date(run.ended_at).toLocaleTimeString()}`}
        </p>
      </header>

      <section className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <Stat
          label={t("methodology.statDispatches", "Dispatches")}
          value={`${run.completed}/${run.planned}`}
          subtle={run.status}
        />
        <Stat
          label={t("methodology.statYourCost", "YOUR cost")}
          value={`$${run.customer_cost_usd.toFixed(4)}`}
          subtle={`${run.customer_tokens_in.toLocaleString()} in / ${run.customer_tokens_out.toLocaleString()} out`}
        />
        <Stat
          label={t("methodology.statOurCost", "OUR cost (delivery)")}
          value={`$${run.provider_total_cost_usd.toFixed(4)}`}
          subtle={
            run.provider_judge_cost_usd > 0
              ? `incl. $${run.provider_judge_cost_usd.toFixed(4)} judge`
              : t("methodology.noJudgeCalls", "no judge calls")
          }
        />
        <Stat
          label={t("methodology.statMeanScore", "Mean score")}
          value={
            overallMeanScore !== null
              ? overallMeanScore.toFixed(3)
              : "—"
          }
          subtle={t("methodology.scoreScale", "0..1 rubric scale")}
        />
      </section>

      <section>
        <h3 className="text-cs-fg font-medium text-sm mb-3">
          {t("methodology.composition", "Per-cell composition")} ({cells.length} cells)
        </h3>
        {cells.length === 0 ? (
          <div className="text-cs-muted text-sm border border-cs-border/30 rounded-lg p-4">
            {t(
              "methodology.noDispatches",
              "No completed dispatches yet in this run.",
            )}
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs border-collapse">
              <thead>
                <tr className="text-cs-muted text-[10px] uppercase tracking-wide border-b border-cs-border/40">
                  <th className="text-left p-2">prompt</th>
                  <th className="text-left p-2">model</th>
                  <th className="text-left p-2">condition</th>
                  <th className="text-right p-2">n</th>
                  <th className="text-right p-2">cost mean</th>
                  <th className="text-right p-2">95% CI</th>
                  <th className="text-right p-2">score mean</th>
                  <th className="text-right p-2">pass ≥0.5</th>
                </tr>
              </thead>
              <tbody>
                {cells.map((c, i) => (
                  <CellRow key={i} cell={c} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <footer className="text-cs-muted text-xs border-t border-cs-border/30 pt-4">
        {t(
          "methodology.footerNote",
          "All numbers above are computed from local SQLite (~/.ato/local.db). Use",
        )}{" "}
        <code className="text-cs-accent">ato evaluations methodology runs show {run.id}</code>{" "}
        {t(
          "methodology.footerNote2",
          "for the full CLI output including Welch t-statistics between models.",
        )}
      </footer>
    </div>
  );
}

function Stat({
  label,
  value,
  subtle,
}: {
  label: string;
  value: string;
  subtle?: string;
}) {
  return (
    <div className="border border-cs-border/30 bg-cs-card/30 rounded-lg p-3">
      <div className="text-[10px] uppercase tracking-wide text-cs-muted font-semibold">
        {label}
      </div>
      <div className="text-cs-fg font-mono text-lg mt-1">{value}</div>
      {subtle && <div className="text-cs-muted text-xs mt-1">{subtle}</div>}
    </div>
  );
}

function CellRow({ cell }: { cell: CellSummary }) {
  const trend = cell.score
    ? cell.score.mean >= 0.7
      ? "up"
      : cell.score.mean < 0.3
      ? "down"
      : null
    : null;
  return (
    <tr className="border-b border-cs-border/20 hover:bg-cs-card/30">
      <td className="p-2 text-cs-fg/80">[{cell.promptIdx}]</td>
      <td className="p-2 font-mono text-cs-accent/80">{cell.model}</td>
      <td className="p-2 text-cs-muted">{cell.condition}</td>
      <td className="p-2 text-right text-cs-fg/80">{cell.n}</td>
      <td className="p-2 text-right font-mono text-cs-fg/80">
        ${cell.cost.mean.toFixed(4)}
      </td>
      <td className="p-2 text-right font-mono text-cs-muted text-[11px]">
        ${cell.cost.ciLo.toFixed(4)}…${cell.cost.ciHi.toFixed(4)}
      </td>
      <td className="p-2 text-right font-mono">
        {cell.score ? (
          <span
            className={cn(
              "inline-flex items-center gap-1",
              trend === "up" && "text-green-400",
              trend === "down" && "text-red-400",
              trend === null && "text-cs-fg/80",
            )}
          >
            {trend === "up" && <TrendingUp className="w-3 h-3" />}
            {trend === "down" && <TrendingDown className="w-3 h-3" />}
            {cell.score.mean.toFixed(3)}
          </span>
        ) : (
          <span className="text-cs-muted">—</span>
        )}
      </td>
      <td className="p-2 text-right text-cs-fg/80">
        {cell.passedAt05 !== null ? (
          <span className="inline-flex items-center gap-1">
            {cell.passedAt05 === cell.n && (
              <CheckCircle2 className="w-3 h-3 text-green-400" />
            )}
            {cell.passedAt05}/{cell.n}
          </span>
        ) : (
          <span className="text-cs-muted">—</span>
        )}
      </td>
    </tr>
  );
}
