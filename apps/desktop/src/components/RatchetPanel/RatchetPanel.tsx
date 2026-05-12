// v2.3.45 — Ratchet panel for Insights.
//
// Visualizes the Phase 6.x-K eval-score ratchet: every locked floor,
// current 7-day rate per target, breach history. Lock/unlock from
// the GUI involves spawning the CLI (not in this slice — empty-state
// docs the `ato ratchet lock` command). The panel is read-only and
// refreshes every 30s so breaches that ops recipes fire on (via the
// events bus) land here without a manual reload.

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  Lock,
  AlertCircle,
  CheckCircle2,
  MinusCircle,
  History,
  Plus,
  X,
  Trash2,
  Loader2,
} from "lucide-react";
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

// v2.3.49 — single source of truth for the target-string format
// the CLI's parse_target accepts. Used by both the display label,
// the unlock IPC call, and the disabled-button key so a future
// format change propagates to all three sites.
function targetKey(row: { targetKind: string; targetValue: string }) {
  if (row.targetKind === "global") return "global";
  return `${row.targetKind}:${row.targetValue}`;
}
function targetDisp(row: { targetKind: string; targetValue: string }) {
  return targetKey(row);
}

function percent(v: number | null | undefined) {
  if (v == null) return "—";
  return `${(v * 100).toFixed(1)}%`;
}

export default function RatchetPanel() {
  const queryClient = useQueryClient();
  const [showLock, setShowLock] = useState(false);
  const [unlocking, setUnlocking] = useState<string | null>(null);

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

  const handleUnlock = async (row: RatchetRow) => {
    const target = targetKey(row);
    if (
      !window.confirm(
        `Unlock ratchet for ${target}? The floor will be removed; ato ratchet check no longer fails on this target.`,
      )
    ) {
      return;
    }
    setUnlocking(target);
    try {
      await invoke("unlock_ratchet", { target });
      await queryClient.invalidateQueries({ queryKey: ["ratchets-list"] });
    } catch (e) {
      window.alert(`Unlock failed: ${e}`);
    } finally {
      setUnlocking(null);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Lock className="text-cs-accent" size={24} />
            Eval-score ratchet
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            Locked quality floors per target. Each row's current 7-day rate must stay above
            <code className="mx-1 px-1 rounded bg-cs-card">floor − threshold</code>.
            <code className="mx-1 px-1 rounded bg-cs-card">ato ratchet check</code> exits non-zero
            on any breach — drop into CI as a deploy gate.
          </p>
        </div>
        <button
          onClick={() => setShowLock(true)}
          className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90"
        >
          <Plus size={14} /> Lock floor
        </button>
      </div>

      {showLock && (
        <LockFloorModal
          onClose={() => setShowLock(false)}
          onLocked={async () => {
            setShowLock(false);
            await queryClient.invalidateQueries({ queryKey: ["ratchets-list"] });
          }}
        />
      )}

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
                  <div className="mt-2 flex items-center justify-between gap-3">
                    <div className="text-[10px] text-cs-muted">
                      metric: {r.metric} · window: {r.baselineWindowDays}d · locked{" "}
                      {new Date(r.lockedAt).toLocaleDateString()}
                    </div>
                    <button
                      onClick={() => handleUnlock(r)}
                      disabled={unlocking === targetKey(r)}
                      className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-danger disabled:opacity-50"
                      title="Remove this lock"
                    >
                      <Trash2 size={10} /> unlock
                    </button>
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

const TARGET_KINDS = ["runtime", "agent", "global"] as const;
const RUNTIME_OPTIONS = [
  "claude",
  "codex",
  "gemini",
  "hermes",
  "openclaw",
  "minimax",
  "grok",
  "deepseek",
  "qwen",
  "openrouter",
];

function LockFloorModal({
  onClose,
  onLocked,
}: {
  onClose: () => void;
  // Accept Promise-returning handlers because the caller awaits a
  // queryClient.invalidateQueries before closing the modal. Earlier
  // type was `() => void` which silently dropped the rejection.
  onLocked: () => Promise<void>;
}) {
  const [kind, setKind] = useState<(typeof TARGET_KINDS)[number]>("runtime");
  const [runtimeValue, setRuntimeValue] = useState("claude");
  const [agentValue, setAgentValue] = useState("");
  const [days, setDays] = useState(30);
  const [thresholdPp, setThresholdPp] = useState(5);
  const [notes, setNotes] = useState("");
  const [locking, setLocking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const buildTarget = () => {
    if (kind === "global") return "global";
    if (kind === "runtime") return `runtime:${runtimeValue}`;
    return `agent:${agentValue.trim()}`;
  };

  const handleLock = async () => {
    if (kind === "agent") {
      const slug = agentValue.trim();
      if (!slug) {
        setError("Agent slug is required.");
        return;
      }
      // v2.3.49 — agent_slug values land in `ato ratchet lock --target
      // agent:<slug>` and from there into the eval_ratchets PK. Match
      // the format `execution_logs.agent_slug` already uses (lowercase
      // ASCII + dash) so the lookup is consistent. The Rust side
      // bounds-checks integer ranges but not the slug shape.
      if (!/^[a-z0-9][a-z0-9-]*$/.test(slug)) {
        setError("Agent slug must be lowercase ASCII + dashes, starting with a letter or digit.");
        return;
      }
    }
    setLocking(true);
    setError(null);
    try {
      await invoke("lock_ratchet", {
        target: buildTarget(),
        days,
        threshold: thresholdPp / 100,
        notes: notes.trim() || null,
      });
      await onLocked();
    } catch (e) {
      setError(String(e));
    } finally {
      setLocking(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="relative bg-cs-card border border-cs-border rounded-lg p-6 w-full max-w-md space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          onClick={onClose}
          className="absolute top-3 right-3 text-cs-muted hover:text-cs-text"
          aria-label="close"
        >
          <X size={16} />
        </button>
        <h3 className="text-lg font-semibold text-cs-text">Lock a quality floor</h3>
        <p className="text-xs text-cs-muted">
          Computes the current success rate over the last N days and persists it as a floor.
          <code className="ml-1 bg-cs-bg px-1 rounded">ato ratchet check</code> exits non-zero in
          CI when the rate drops below floor − threshold.
        </p>

        <div className="space-y-3">
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Target kind</label>
            <div className="mt-1 flex gap-2">
              {TARGET_KINDS.map((k) => (
                <button
                  key={k}
                  type="button"
                  onClick={() => setKind(k)}
                  className={cn(
                    "px-3 py-1.5 rounded-md text-xs font-medium border transition-colors",
                    kind === k
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border text-cs-muted hover:bg-cs-border/30",
                  )}
                >
                  {k}
                </button>
              ))}
            </div>
          </div>

          {kind === "runtime" && (
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">Runtime</label>
              <select
                value={runtimeValue}
                onChange={(e) => setRuntimeValue(e.target.value)}
                className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
              >
                {RUNTIME_OPTIONS.map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </select>
            </div>
          )}

          {kind === "agent" && (
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">Agent slug</label>
              <input
                type="text"
                value={agentValue}
                onChange={(e) => setAgentValue(e.target.value)}
                placeholder="e.g. triage-bot"
                className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
              />
              <div className="mt-1 text-[10px] text-cs-muted">
                Must match the <code>agent_slug</code> stored on <code>execution_logs</code>.
                Only rows tagged via <code>--agent</code> on dispatch contribute.
              </div>
            </div>
          )}

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">
                Baseline window
              </label>
              <div className="mt-1 flex items-center gap-2">
                <input
                  type="number"
                  min={1}
                  max={365}
                  value={days}
                  onChange={(e) => setDays(parseInt(e.target.value || "30", 10))}
                  className="w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
                />
                <span className="text-xs text-cs-muted">days</span>
              </div>
            </div>
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">Threshold</label>
              <div className="mt-1 flex items-center gap-2">
                <input
                  type="number"
                  min={0}
                  max={100}
                  step={1}
                  value={thresholdPp}
                  onChange={(e) =>
                    setThresholdPp(Math.max(0, Math.min(100, parseInt(e.target.value || "5", 10))))
                  }
                  className="w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
                />
                <span className="text-xs text-cs-muted">pp</span>
              </div>
            </div>
          </div>

          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Notes (optional)</label>
            <input
              type="text"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder="e.g. minimax review-quality floor for Q3"
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            />
          </div>
        </div>

        {error && <div className="text-xs text-cs-danger">{error}</div>}

        <div className="flex items-center justify-end gap-2">
          <button
            onClick={onClose}
            disabled={locking}
            className="px-3 py-2 rounded-md border border-cs-border text-sm hover:bg-cs-border/30"
          >
            Cancel
          </button>
          <button
            onClick={handleLock}
            disabled={locking}
            className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90 disabled:opacity-40"
          >
            {locking ? <Loader2 size={14} className="animate-spin" /> : <Lock size={14} />}
            Lock
          </button>
        </div>
      </div>
    </div>
  );
}
