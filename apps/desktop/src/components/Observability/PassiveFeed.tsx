// v2.13 — Universal multi-LLM observability surface.
//
// Renders the rows the passive watcher writes into execution_logs
// (dispatch_kind = 'passive_observation'). Distinct from LiveRuns,
// which mixes ATO's own dispatches with passive observations: this
// feed is "what every other CLI on this machine has been doing,"
// chronologically. The History panel shows the same data inline with
// active runs; this one isolates the observed slice so users can spot
// usage outside ATO without mental filtering.
//
// Data flow:
//   Rust passive_observer (auto-start) → execution_logs SQLite →
//   Tauri command `list_passive_observations` (apps/desktop/src-tauri/
//   src/observe.rs) → this component polls every 5s via React Query.
//
// Privacy: rows are local SQLite only. Cloud aggregation lives in
// ato-cloud's services/observability-ingest and is opt-in / Pro-tier.

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Eye, Loader2, AlertCircle, Cpu, Clock } from "lucide-react";
import {
  billingSurfaceLabel,
  billingSurfaceShortLabel,
  type ActiveRun,
} from "@/lib/activeRuns";

export interface PassiveObservation {
  id: string;
  runtime: string;
  model: string | null;
  prompt: string | null;
  response: string | null;
  tokens_in: number | null;
  tokens_out: number | null;
  cost_usd_estimated: number | null;
  billing_surface: ActiveRun["billing_surface"];
  provider_session_id: string | null;
  sequence_within_session: number | null;
  created_at: string;
}

export interface ObserverStatus {
  running: boolean;
  sources: string[];
}

async function listPassiveObservations(
  limit = 100,
  runtime?: string,
): Promise<PassiveObservation[]> {
  return invoke<PassiveObservation[]>("list_passive_observations", {
    limit,
    runtime: runtime ?? null,
  });
}

async function getObserverStatus(): Promise<ObserverStatus> {
  return invoke<ObserverStatus>("get_observer_status");
}

function formatRelative(iso: string): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const seconds = Math.floor((Date.now() - then) / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

function formatCost(cost: number | null): string {
  if (cost == null) return "—";
  if (cost < 0.001) return "<$0.001";
  return `$${cost.toFixed(cost < 0.01 ? 4 : 3)}`;
}

export default function PassiveFeed() {
  const statusQ = useQuery({
    queryKey: ["observer-status"],
    queryFn: getObserverStatus,
    refetchInterval: 10_000,
  });

  const feedQ = useQuery({
    queryKey: ["passive-observations"],
    queryFn: () => listPassiveObservations(200),
    refetchInterval: 5_000,
    staleTime: 0,
  });

  const totals = useMemo(() => {
    const rows = feedQ.data ?? [];
    const tokensIn = rows.reduce((a, r) => a + (r.tokens_in ?? 0), 0);
    const tokensOut = rows.reduce((a, r) => a + (r.tokens_out ?? 0), 0);
    const cost = rows.reduce((a, r) => a + (r.cost_usd_estimated ?? 0), 0);
    const byRuntime = rows.reduce<Record<string, number>>((acc, r) => {
      acc[r.runtime] = (acc[r.runtime] ?? 0) + 1;
      return acc;
    }, {});
    return { tokensIn, tokensOut, cost, byRuntime, count: rows.length };
  }, [feedQ.data]);

  if (feedQ.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        Loading observations…
      </div>
    );
  }

  if (feedQ.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>Couldn&apos;t load observations: {String(feedQ.error)}</span>
      </div>
    );
  }

  const rows = feedQ.data ?? [];
  const sources = statusQ.data?.sources ?? [];

  return (
    <div className="space-y-4">
      <header className="space-y-2">
        <div className="flex items-center gap-2">
          <Eye size={16} className="text-cs-accent" />
          <h2 className="text-sm font-semibold text-cs-text">
            Multi-LLM passive observations
          </h2>
        </div>
        <p className="text-xs text-cs-muted leading-relaxed">
          Every Claude Code, Codex, and Gemini CLI session this machine has
          run — outside ATO — captured locally so you can see your full LLM
          usage in one place. Read-only; no network upload unless you opt
          in to cloud sync.
        </p>
        <ObserverStatusBadge status={statusQ.data} />
      </header>

      <Totals
        count={totals.count}
        tokensIn={totals.tokensIn}
        tokensOut={totals.tokensOut}
        cost={totals.cost}
        byRuntime={totals.byRuntime}
      />

      {sources.length === 0 ? (
        <div className="rounded-lg border border-cs-border bg-cs-surface p-4 text-xs text-cs-muted">
          No supported CLI directories detected yet
          (~/.claude/projects, ~/.codex/sessions, ~/.gemini). Install at
          least one to start observing.
        </div>
      ) : rows.length === 0 ? (
        <div className="rounded-lg border border-cs-border bg-cs-surface p-4 text-xs text-cs-muted">
          No observed dispatches yet. Fire any prompt from your installed
          CLI ({sources.join(", ")}) and it&apos;ll appear here within a
          few seconds.
        </div>
      ) : (
        <ul className="space-y-2">
          {rows.map((row) => (
            <ObservationRow key={row.id} row={row} />
          ))}
        </ul>
      )}
    </div>
  );
}

function ObserverStatusBadge({ status }: { status?: ObserverStatus }) {
  if (!status) {
    return null;
  }
  const dotColor = status.running ? "bg-cs-accent" : "bg-cs-muted";
  return (
    <div className="flex items-center gap-2 text-xs text-cs-muted">
      <span className={`inline-block w-1.5 h-1.5 rounded-full ${dotColor}`} />
      <span>
        Watcher {status.running ? "running" : "idle"} —{" "}
        {status.sources.length === 0
          ? "no sources detected"
          : `tracking ${status.sources.join(", ")}`}
      </span>
    </div>
  );
}

function Totals({
  count,
  tokensIn,
  tokensOut,
  cost,
  byRuntime,
}: {
  count: number;
  tokensIn: number;
  tokensOut: number;
  cost: number;
  byRuntime: Record<string, number>;
}) {
  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-2">
      <Stat label="Observed" value={`${count}`} />
      <Stat label="Tokens in" value={tokensIn.toLocaleString()} />
      <Stat label="Tokens out" value={tokensOut.toLocaleString()} />
      <Stat label="API-equiv spend" value={formatCost(cost)} />
      <div className="col-span-2 md:col-span-4 flex flex-wrap gap-1 text-xs text-cs-muted">
        {Object.entries(byRuntime).map(([rt, n]) => (
          <span
            key={rt}
            className="px-2 py-0.5 rounded bg-cs-surface border border-cs-border"
          >
            {rt}: {n}
          </span>
        ))}
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-cs-border bg-cs-surface p-2">
      <div className="text-[10px] uppercase tracking-wide text-cs-muted">
        {label}
      </div>
      <div className="text-sm text-cs-text font-medium">{value}</div>
    </div>
  );
}

function ObservationRow({ row }: { row: PassiveObservation }) {
  const surfaceShort = billingSurfaceShortLabel(row.billing_surface);
  const surfaceFull = billingSurfaceLabel(row.billing_surface);
  return (
    <li className="rounded-lg border border-cs-border bg-cs-surface p-3 text-xs">
      <div className="flex items-center justify-between gap-2 mb-1">
        <div className="flex items-center gap-2">
          <Cpu size={12} className="text-cs-muted shrink-0" />
          <span className="font-medium text-cs-text">{row.runtime}</span>
          {row.model && (
            <span className="text-cs-muted">· {row.model}</span>
          )}
        </div>
        <div className="flex items-center gap-2 text-cs-muted">
          <Clock size={11} />
          <span>{formatRelative(row.created_at)}</span>
        </div>
      </div>

      {row.prompt && (
        <div className="text-cs-text mb-1 line-clamp-2">
          <span className="text-cs-muted">→ </span>
          {row.prompt.slice(0, 240)}
          {row.prompt.length > 240 ? "…" : ""}
        </div>
      )}
      {row.response && (
        <div className="text-cs-muted line-clamp-2">
          <span>← </span>
          {row.response.slice(0, 240)}
          {row.response.length > 240 ? "…" : ""}
        </div>
      )}

      <div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] text-cs-muted">
        <span
          className="px-1.5 py-0.5 rounded bg-cs-bg border border-cs-border"
          title={surfaceFull}
        >
          {surfaceShort}
        </span>
        {row.tokens_in != null && row.tokens_out != null && (
          <span>
            {row.tokens_in.toLocaleString()} in / {row.tokens_out.toLocaleString()} out
          </span>
        )}
        {row.cost_usd_estimated != null && (
          <span>{formatCost(row.cost_usd_estimated)} (est)</span>
        )}
        {row.provider_session_id && (
          <span className="text-cs-muted/60 font-mono">
            {row.provider_session_id.slice(0, 8)}…#{row.sequence_within_session}
          </span>
        )}
      </div>
    </li>
  );
}
