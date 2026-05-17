// PR 5c (Sessions UX polish, 2026-05-17) — single-shot dispatch
// detail view. A session uuid and an execution_log uuid live in the
// same string space, so the parent SessionsList encodes a discriminator
// (kind: "session" | "ephemeral") alongside the open id to route
// correctly. This view is intentionally lighter than SessionTranscriptView:
// one prompt + one response, no Continue / Bridge / Close affordances
// (ephemeral by definition — there's nothing to continue).
//
// Extracted from SessionsList.tsx per codex-reviewer Round-1 #3:
// inlining a full detail view in a ~2k-line parent was a readability
// tax. Helpers (runtimeBadge / personaBadge / personaDisplay /
// formatTime) are re-exported from the parent file so this view shares
// the exact same styling treatment as the rest of the Sessions tab.

import { Loader2 } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import {
  runtimeBadge,
  personaBadge,
  personaDisplay,
  formatTime,
} from "./_helpers";

export interface EphemeralDetail {
  id: string;
  runtime: string;
  agentSlug: string | null;
  model: string | null;
  status: string;
  prompt: string | null;
  response: string | null;
  errorMessage: string | null;
  createdAt: string;
  durationMs: number | null;
  tokensIn: number | null;
  tokensOut: number | null;
  costUsdEstimated: number | null;
  authMode: string | null;
}

export default function EphemeralDetailView({
  logId,
  onBack,
}: {
  logId: string;
  onBack: () => void;
}) {
  const q = useQuery<EphemeralDetail>({
    queryKey: ["ephemeral-detail", logId],
    queryFn: () =>
      invoke<EphemeralDetail>("get_ephemeral_detail", { logId }),
    staleTime: 60_000,
  });

  if (q.isLoading) {
    return (
      <div className="flex items-center justify-center h-32">
        <Loader2 className="animate-spin text-cs-accent" size={28} />
      </div>
    );
  }
  if (q.isError || !q.data) {
    return (
      <div className="space-y-4">
        <button
          onClick={onBack}
          className="text-sm text-cs-muted hover:text-cs-text"
        >
          ← Back to Sessions
        </button>
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-4 text-sm text-cs-text">
          Could not load dispatch detail
          {q.error instanceof Error ? `: ${q.error.message}` : ""}.
        </div>
      </div>
    );
  }
  const d = q.data;
  const isErr = d.status !== "success";
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <button
          onClick={onBack}
          className="text-sm text-cs-muted hover:text-cs-text"
        >
          ← Back to Sessions
        </button>
        <div className="text-xs text-cs-muted font-mono">{d.id}</div>
      </div>

      <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-3">
        <div className="flex flex-wrap items-center gap-3">
          <span className={runtimeBadge(d.runtime)}>{d.runtime}</span>
          {d.agentSlug && (
            <span className={personaBadge()}>
              {personaDisplay(d.agentSlug)}
            </span>
          )}
          <span
            className={cn(
              "px-1.5 py-0.5 rounded text-[10px] font-medium uppercase",
              isErr
                ? "bg-cs-danger/15 text-cs-danger"
                : "bg-cs-muted/15 text-cs-muted"
            )}
          >
            single run · {d.status}
          </span>
          {d.model && (
            <span className="text-xs text-cs-muted font-mono">{d.model}</span>
          )}
          <span className="text-xs text-cs-muted ml-auto">
            {formatTime(d.createdAt)}
          </span>
        </div>
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-cs-muted">
          {d.durationMs !== null && (
            <span>
              duration:{" "}
              <span className="text-cs-text font-mono">
                {(d.durationMs / 1000).toFixed(2)}s
              </span>
            </span>
          )}
          {(d.tokensIn !== null || d.tokensOut !== null) && (
            <span>
              tokens:{" "}
              <span className="text-cs-text font-mono">
                {d.tokensIn ?? 0} / {d.tokensOut ?? 0}
              </span>
            </span>
          )}
          {d.costUsdEstimated !== null && d.costUsdEstimated > 0 && (
            <span>
              cost:{" "}
              <span className="text-cs-text font-mono">
                ${d.costUsdEstimated.toFixed(4)}
              </span>
            </span>
          )}
          {d.authMode && (
            <span>
              auth:{" "}
              <span className="text-cs-text font-mono">{d.authMode}</span>
            </span>
          )}
        </div>
      </div>

      <div className="space-y-3">
        <div className="rounded-lg border border-cs-border bg-cs-card p-4">
          <div className="text-[10px] uppercase tracking-wider text-cs-muted font-medium mb-2">
            Prompt
          </div>
          <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
            {d.prompt ?? "(no prompt recorded)"}
          </pre>
        </div>

        <div className="rounded-lg border border-cs-border bg-cs-card p-4">
          <div className="text-[10px] uppercase tracking-wider text-cs-muted font-medium mb-2">
            Response
          </div>
          <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
            {d.response ?? "(no response recorded)"}
          </pre>
        </div>

        {d.errorMessage && (
          <div className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-4">
            <div className="text-[10px] uppercase tracking-wider text-cs-danger font-medium mb-2">
              Error
            </div>
            <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
              {d.errorMessage}
            </pre>
          </div>
        )}
      </div>
    </div>
  );
}
