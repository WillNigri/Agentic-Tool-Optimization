// v2.3.45 — Ratchet panel for Insights.
//
// Visualizes the Phase 6.x-K eval-score ratchet: every locked floor,
// current 7-day rate per target, breach history. Lock/unlock from
// the GUI involves spawning the CLI (not in this slice — empty-state
// docs the `ato ratchet lock` command). The panel is read-only and
// refreshes every 30s so breaches that ops recipes fire on (via the
// events bus) land here without a manual reload.

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Lock, AlertCircle, CheckCircle2, MinusCircle, History } from "lucide-react";
import { cn } from "@/lib/utils";

interface RatchetRow {
  targetKind: string;
  targetValue: string;
  metric: string;
  baselineValue: number;
  baselineWindowDays: number;
  threshold: number;
  currentValue: number | null;
  currentSampleCount: number;
  floorWithTolerance: number;
  verdict: "pass" | "fail" | "insufficient_data";
  lockedAt: string;
  notes: string | null;
}

interface BreachEvent {
  eventSeq: number;
  targetKind: string;
  targetValue: string;
  baselineValue: number;
  currentValue: number;
  currentSampleCount: number;
  occurredAt: string;
}

function targetDisp(row: { targetKind: string; targetValue: string }) {
  if (row.targetKind === "global") return "global";
  return `${row.targetKind}:${row.targetValue}`;
}

function percent(v: number | null | undefined) {
  if (v == null) return "—";
  return `${(v * 100).toFixed(1)}%`;
}

export default function RatchetPanel() {
  const ratchetsQ = useQuery<RatchetRow[]>({
    queryKey: ["ratchets-list"],
    queryFn: () => invoke<RatchetRow[]>("list_ratchets"),
    refetchInterval: 30_000,
  });

  const breachesQ = useQuery<BreachEvent[]>({
    queryKey: ["ratchet-breaches"],
    queryFn: () => invoke<BreachEvent[]>("list_ratchet_breaches", { limit: 20 }),
    refetchInterval: 30_000,
  });

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold flex items-center gap-2">
          <Lock className="text-cs-accent" size={24} />
          Eval-score ratchet
        </h2>
        <p className="text-sm text-cs-muted mt-1">
          Locked quality floors per target. Each row's current 7-day rate must stay above
          <code className="mx-1 px-1 rounded bg-cs-card">floor − threshold</code>.
          <code className="mx-1 px-1 rounded bg-cs-card">ato ratchet check</code> exits non-zero on
          any breach — drop into CI as a deploy gate.
        </p>
      </div>

      <section>
        <h3 className="text-sm font-medium text-cs-text mb-2">Locked floors</h3>
        {!ratchetsQ.data || ratchetsQ.data.length === 0 ? (
          <div className="border border-cs-border rounded-lg bg-cs-card p-6 text-sm text-cs-muted">
            <p>No ratchets locked yet.</p>
            <p className="mt-2 text-xs">
              Lock one with{" "}
              <code className="bg-cs-bg px-1.5 py-0.5 rounded text-cs-text">
                ato ratchet lock --target runtime:claude --days 30
              </code>
              . The check uses the last <code>--days</code> as the baseline, then enforces a
              5pp tolerance (override via <code>--threshold</code>).
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {ratchetsQ.data.map((r) => {
              const verdictMeta = {
                pass: { Icon: CheckCircle2, color: "text-cs-accent", label: "Pass" },
                fail: { Icon: AlertCircle, color: "text-cs-danger", label: "FAIL" },
                insufficient_data: {
                  Icon: MinusCircle,
                  color: "text-cs-muted",
                  label: "Insufficient data",
                },
              }[r.verdict];
              const { Icon, color, label } = verdictMeta;
              return (
                <div
                  key={`${r.targetKind}-${r.targetValue}-${r.metric}`}
                  className={cn(
                    "border rounded-lg p-4",
                    r.verdict === "fail"
                      ? "border-cs-danger/40 bg-cs-danger/5"
                      : "border-cs-border bg-cs-card"
                  )}
                >
                  <div className="flex items-center gap-3 flex-wrap">
                    <Icon size={16} className={color} />
                    <span className="text-sm font-medium text-cs-text">{targetDisp(r)}</span>
                    <span className={cn("text-xs uppercase font-medium", color)}>{label}</span>
                    <div className="ml-auto flex items-center gap-4 text-xs text-cs-muted">
                      <span>
                        floor: <strong className="text-cs-text">{percent(r.baselineValue)}</strong>
                      </span>
                      <span>
                        floor − tol:{" "}
                        <strong className="text-cs-text">{percent(r.floorWithTolerance)}</strong>
                      </span>
                      <span>
                        current 7d:{" "}
                        <strong
                          className={cn(
                            r.verdict === "fail" ? "text-cs-danger" : "text-cs-text"
                          )}
                        >
                          {percent(r.currentValue)}
                        </strong>{" "}
                        <span className="opacity-60">({r.currentSampleCount} samples)</span>
                      </span>
                    </div>
                  </div>
                  {r.notes && (
                    <div className="mt-2 text-xs text-cs-muted">{r.notes}</div>
                  )}
                  <div className="mt-2 text-[10px] text-cs-muted">
                    metric: {r.metric} · window: {r.baselineWindowDays}d · locked{" "}
                    {new Date(r.lockedAt).toLocaleDateString()}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section>
        <h3 className="text-sm font-medium text-cs-text mb-2 flex items-center gap-2">
          <History size={14} />
          Breach history
        </h3>
        {!breachesQ.data || breachesQ.data.length === 0 ? (
          <div className="border border-cs-border rounded-lg bg-cs-card p-4 text-xs text-cs-muted">
            No breaches recorded. v2.3.40 publishes <code>ratchet_breach</code> events to{" "}
            <code>events_log</code> whenever <code>ato ratchet check</code> fails; this list shows
            the 20 most recent.
          </div>
        ) : (
          <div className="space-y-1">
            {breachesQ.data.map((b) => (
              <div
                key={b.eventSeq}
                className="border border-cs-border rounded-md bg-cs-card px-3 py-2 text-xs flex items-center gap-3"
              >
                <AlertCircle size={12} className="text-cs-danger shrink-0" />
                <span className="font-medium text-cs-text">{targetDisp(b)}</span>
                <span className="text-cs-muted">
                  current {percent(b.currentValue)} vs floor {percent(b.baselineValue)}
                </span>
                <span className="text-cs-muted">{b.currentSampleCount} samples</span>
                <span className="ml-auto text-[10px] text-cs-muted">
                  {new Date(b.occurredAt).toLocaleString()}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
