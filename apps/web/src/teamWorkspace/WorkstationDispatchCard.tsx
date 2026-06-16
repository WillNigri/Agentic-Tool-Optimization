import { useState } from "react";
import {
  CheckCircle2,
  LoaderCircle,
  Square,
  Terminal,
  XCircle,
} from "lucide-react";
import type { AgentRuntime } from "@/components/cron/types";
import { RUNTIME_ENTRIES } from "@/lib/runtimes";
import * as tetherClient from "../lib/tether/client";
import { useDispatchRequest } from "../lib/tether/dispatchClient";

interface Props {
  machineName: string;
}

const WAVE2_ENABLED_RUNTIMES = ["claude", "codex", "gemini", "openclaw", "hermes"] as const;
const NEXT_WAVE_RUNTIMES = ["minimax", "grok", "deepseek", "qwen", "openrouter"] as const;

function formatCost(costUsd?: number | null): string {
  if (costUsd === null || costUsd === undefined) return "—";
  return `$${costUsd.toFixed(costUsd > 0 && costUsd < 0.01 ? 4 : 3)}`;
}

function formatDuration(durationMs?: number | null): string {
  if (durationMs === null || durationMs === undefined) return "—";
  return `${(durationMs / 1000).toFixed(1)}s`;
}

function statusTone(status: string): string {
  switch (status) {
    case "success":
      return "text-[#00FFB2] bg-[#00FFB2]/10 border-[#00FFB2]/20";
    case "failed":
    case "denied":
    case "cancelled":
      return "text-red-400 bg-red-500/10 border-red-500/20";
    case "cancelling":
      return "text-yellow-300 bg-yellow-500/10 border-yellow-500/20";
    default:
      return "text-[#8888a0] bg-[#2a2a3a]/50 border-[#2a2a3a]";
  }
}

function isWave2EnabledRuntime(id: string): id is AgentRuntime {
  return WAVE2_ENABLED_RUNTIMES.includes(id as (typeof WAVE2_ENABLED_RUNTIMES)[number]);
}

function isNextWaveRuntime(id: string): boolean {
  return NEXT_WAVE_RUNTIMES.includes(id as (typeof NEXT_WAVE_RUNTIMES)[number]);
}

export default function WorkstationDispatchCard({ machineName }: Props) {
  const { send, current } = useDispatchRequest(tetherClient);
  const [runtime, setRuntime] = useState<AgentRuntime>("claude");
  const [prompt, setPrompt] = useState("");
  const [sendError, setSendError] = useState<string | null>(null);

  const isRunning = current?.status === "running" || current?.status === "cancelling";
  const responseText = current?.chunks.join("") ?? "";
  const shortRequestId = current?.requestId.slice(0, 8) ?? null;
  const isStreaming = isRunning && (current?.chunks.length ?? 0) > 1;

  function handleSend(): void {
    const nextPrompt = prompt.trim();
    if (!nextPrompt) return;

    try {
      setSendError(null);
      send({ runtime, prompt: nextPrompt, model: null, agent_slug: null });
      setPrompt("");
    } catch (err) {
      setSendError(err instanceof Error ? err.message : "Failed to send dispatch");
    }
  }

  function handleCancel(): void {
    if (!current?.requestId || !isRunning) return;
    send({ kind: "dispatch_cancel", request_id: current.requestId });
  }

  return (
    <div className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-4 space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-white">
              {isRunning ? `Running on ${machineName}` : `Run on ${machineName}`}
            </h3>
            {isStreaming && (
              <span className="text-xs text-[#8888a0]">streaming…</span>
            )}
          </div>
          <p className="text-xs text-[#8888a0]">
            Pair-level auth is active for this workstation.
          </p>
        </div>
        {current?.requestId && isRunning && (
          <button
            type="button"
            onClick={handleCancel}
            className="inline-flex items-center gap-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-sm font-semibold text-red-300 transition-colors hover:border-red-400/40 hover:bg-red-500/15"
          >
            <Square className="h-4 w-4" />
            Cancel
          </button>
        )}
      </div>

      {!isRunning && (
        <>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-2">
              <label className="text-[11px] font-semibold uppercase tracking-wider text-[#8888a0]">
                Runtime
              </label>
              <div className="grid gap-2">
                {RUNTIME_ENTRIES.filter(([id]) =>
                  isWave2EnabledRuntime(id) || isNextWaveRuntime(id),
                ).map(([id, meta]) => {
                  const enabled = isWave2EnabledRuntime(id);
                  const selected = runtime === id;
                  const Icon = meta.icon;
                  return (
                    <button
                      key={id}
                      type="button"
                      disabled={!enabled}
                      onClick={() => {
                        if (enabled) setRuntime(id);
                      }}
                      className={`flex items-center justify-between rounded-lg border px-3 py-2 text-left transition-colors ${
                        selected
                          ? "border-orange-500/30 bg-orange-500/10 text-white"
                          : enabled
                            ? "border-[#2a2a3a] bg-[#0f0f15] text-[#d7d7e0] hover:border-[#3a3a4a] hover:bg-[#14141c]"
                            : "border-[#2a2a3a] bg-[#0f0f15] text-[#66667a]"
                      }`}
                    >
                      <span className="flex items-center gap-2 text-sm font-medium">
                        <Icon className="h-4 w-4" />
                        {meta.label}
                      </span>
                      {enabled ? (
                        <span className={`rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase ${
                          selected
                            ? "bg-orange-500/20 text-orange-300"
                            : "bg-[#2a2a3a] text-[#aaaab8]"
                        }`}>
                          Wave 2
                        </span>
                      ) : (
                        <span className="rounded-full bg-[#2a2a3a] px-2 py-0.5 text-[10px] font-semibold uppercase text-[#8888a0]">
                          Next wave
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="space-y-2">
              <label className="text-[11px] font-semibold uppercase tracking-wider text-[#8888a0]">
                Prompt
              </label>
              <textarea
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                rows={8}
                placeholder="Send a prompt to your paired workstation..."
                className="min-h-[224px] w-full rounded-lg border border-[#2a2a3a] bg-[#0a0a0f] px-3 py-2 text-sm text-[#e8e8f0] outline-none transition-colors placeholder:text-[#66667a] focus:border-[#00FFB2]/40"
              />
            </div>
          </div>

          {sendError && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
              {sendError}
            </div>
          )}

          <div className="flex justify-end">
            <button
              type="button"
              onClick={handleSend}
              disabled={!prompt.trim()}
              className="inline-flex items-center gap-2 rounded-md bg-[#00FFB2] px-3 py-2 text-sm font-semibold text-[#06110d] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Terminal className="h-4 w-4" />
              Send
            </button>
          </div>
        </>
      )}

      {current && (
        <div className="space-y-3 rounded-lg border border-[#2a2a3a] bg-[#0a0a0f]/70 p-4">
          <div className="flex flex-wrap items-center gap-2">
            {isRunning ? (
              <LoaderCircle className="h-4 w-4 animate-spin text-[#00FFB2]" />
            ) : current.status === "success" ? (
              <CheckCircle2 className="h-4 w-4 text-[#00FFB2]" />
            ) : (
              <XCircle className="h-4 w-4 text-red-400" />
            )}
            <span className="text-sm font-semibold text-white">
              {isRunning ? "Dispatch in flight" : `Completed on ${machineName}`}
            </span>
            {shortRequestId && (
              <span className="rounded-full border border-[#2a2a3a] px-2 py-0.5 font-mono text-[10px] text-[#8888a0]">
                {shortRequestId}
              </span>
            )}
            {!isRunning && (
              <span className={`rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase ${statusTone(current.status)}`}>
                {current.status}
              </span>
            )}
            {!isRunning && current.executionLogId && (
              <a
                href={`/runs/${current.executionLogId}`}
                className="ml-auto text-xs font-medium text-[#00FFB2] hover:text-[#7dffd8]"
              >
                View receipt
              </a>
            )}
          </div>

          {!isRunning && (
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-[#8888a0]">
              <span>
                Model: <span className="font-mono text-[#e8e8f0]">{current.model ?? "default"}</span>
              </span>
              <span>
                Tokens:{" "}
                <span className="font-mono text-[#e8e8f0]">
                  {current.tokensIn ?? 0} in / {current.tokensOut ?? 0} out
                </span>
              </span>
              <span>
                Cost: <span className="font-mono text-[#e8e8f0]">{formatCost(current.costUsd)}</span>
              </span>
              <span>
                Duration: <span className="font-mono text-[#e8e8f0]">{formatDuration(current.durationMs)}</span>
              </span>
            </div>
          )}

          {current.error && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
              {current.error}
            </div>
          )}

          <div className="rounded-lg border border-[#2a2a3a] bg-[#050508] p-3">
            {responseText ? (
              <pre className="whitespace-pre-wrap break-words text-xs leading-relaxed text-[#e8e8f0]">
                {responseText}
              </pre>
            ) : (
              <p className="text-xs text-[#8888a0]">
                {isRunning ? "Waiting for chunks..." : "No response text returned."}
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
