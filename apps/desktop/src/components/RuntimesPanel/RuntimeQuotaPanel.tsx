// v2.13 Phase 6.x polish — Settings → Runtimes → Monitoring quota bars.
//
// Reads each runtime's local quota state via the `ato runtimes status
// --with-quota` CLI surface (shelled out by the Tauri command). When a
// runtime exposes a usage.json the bar shows X / Y messages with a
// reset timestamp; when nothing's discoverable on disk we render
// "quota unknown" honestly + the path we tried. Customer data never
// leaves the machine.
//
// Polls every 60s — usage counters change in the seconds-to-minutes
// range when the user is actively dispatching; faster polling would
// burn IPC for no information.

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Gauge, AlertCircle, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";

interface RuntimeQuotaProbeRow {
  runtime: string;
  sourcePath: string | null;
  found: boolean;
  messagesUsed: number | null;
  messagesLimit: number | null;
  periodResetAt: string | null;
  note: string | null;
}

const REFRESH_MS = 60_000;

export default function RuntimeQuotaPanel() {
  const query = useQuery<RuntimeQuotaProbeRow[]>({
    queryKey: ["runtime-quota-probes"],
    queryFn: () => invoke<RuntimeQuotaProbeRow[]>("list_runtime_quota_probes"),
    refetchInterval: REFRESH_MS,
    staleTime: REFRESH_MS / 2,
  });

  if (query.isLoading) {
    return (
      <div className="flex items-center gap-2 rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 text-xs text-cs-muted">
        <Loader2 size={14} className="animate-spin" />
        Loading runtime quota…
      </div>
    );
  }

  if (query.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>Couldn't load runtime quota: {String(query.error)}</span>
      </div>
    );
  }

  const rows = query.data ?? [];

  return (
    <div className="space-y-3">
      <header>
        <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
          <Gauge size={14} className="text-cs-accent" />
          Runtime quota
        </h3>
        <p className="mt-0.5 text-[11px] text-cs-muted">
          Each runtime's local usage state, read directly from disk. Nothing leaves the machine.
        </p>
      </header>

      {rows.length === 0 ? (
        <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-4 text-center text-xs text-cs-muted">
          No runtime quota probes returned.
        </div>
      ) : (
        <ul className="space-y-2">
          {rows.map((row) => (
            <QuotaRow key={row.runtime} row={row} />
          ))}
        </ul>
      )}
    </div>
  );
}

function QuotaRow({ row }: { row: RuntimeQuotaProbeRow }) {
  const percent =
    row.found && row.messagesUsed != null && row.messagesLimit && row.messagesLimit > 0
      ? Math.min(100, Math.round((row.messagesUsed / row.messagesLimit) * 100))
      : null;

  return (
    <li className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <div className="text-sm font-medium text-cs-text capitalize">{row.runtime}</div>
        {row.found && row.messagesUsed != null && row.messagesLimit != null ? (
          <div className="text-[11px] font-mono text-cs-muted">
            {row.messagesUsed.toLocaleString()} / {row.messagesLimit.toLocaleString()} messages
          </div>
        ) : (
          <div className="text-[11px] text-cs-muted italic">quota unknown</div>
        )}
      </div>

      {percent != null && (
        <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-cs-bg-raised">
          <div
            className={cn(
              "h-full rounded-full transition-all",
              percent >= 90
                ? "bg-cs-danger"
                : percent >= 75
                  ? "bg-cs-warn"
                  : "bg-cs-accent",
            )}
            style={{ width: `${percent}%` }}
            aria-label={`${percent}% used`}
          />
        </div>
      )}

      <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[10px] text-cs-muted">
        {row.periodResetAt && (
          <span>
            resets <time dateTime={row.periodResetAt}>{row.periodResetAt}</time>
          </span>
        )}
        {row.sourcePath && (
          <code className="font-mono" title={row.sourcePath}>
            {row.found ? row.sourcePath : `tried ${row.sourcePath}`}
          </code>
        )}
        {!row.found && row.note && <span className="italic">— {row.note}</span>}
      </div>
    </li>
  );
}
