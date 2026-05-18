// Path A consolidation (2026-05-18) — chat-thread detail view.
//
// The bottom-pane Chat tab is still the live surface for sending +
// receiving chat messages; this view is for *reading* a past chat
// thread that landed in the Sessions feed via the UNION. It's
// intentionally read-only: tapping into a chat from Sessions shows
// the transcript; to continue chatting, the user opens the bottom
// pane (which Path B will turn into a multi-launcher).
//
// Shape mirrors SingleRunDetailView + WarRoomDetailView so the four
// detail views feel like one component family. Same header treatment
// (back button + id chip), same body styling (alternating role
// bubbles).

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

interface ChatMessage {
  id: string;
  threadId: string;
  role: string; // 'user' | 'assistant' | 'system' | 'attachment' | 'error'
  content: string;
  runtime: string | null;
  agentSlug: string | null;
  metadata: string | null;
  createdAt: string;
}

export default function ChatThreadDetailView({
  threadId,
  onBack,
}: {
  threadId: string;
  onBack: () => void;
}) {
  const q = useQuery<ChatMessage[]>({
    queryKey: ["chat-messages", threadId],
    queryFn: () => invoke<ChatMessage[]>("get_chat_messages", { threadId }),
    staleTime: 30_000,
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
          Could not load chat thread
          {q.error instanceof Error ? `: ${q.error.message}` : ""}.
        </div>
      </div>
    );
  }

  const messages = q.data;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <button
          onClick={onBack}
          className="text-sm text-cs-muted hover:text-cs-text"
        >
          ← Back to Sessions
        </button>
        <div className="text-xs text-cs-muted font-mono">{threadId}</div>
      </div>

      <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-1 text-xs text-cs-muted">
        <p>
          🗨 Bottom-pane chat thread · {messages.length} message
          {messages.length !== 1 ? "s" : ""}
        </p>
        <p className="text-[11px]">
          Read-only. To continue this chat, open the bottom pane and pick this
          thread from the dropdown.
        </p>
      </div>

      {messages.length === 0 ? (
        <div className="rounded-md border border-cs-border/60 bg-cs-card/40 p-4 text-sm text-cs-muted">
          This thread has no messages yet.
        </div>
      ) : (
        <div className="space-y-3">
          {messages.map((m) => {
            const isUser = m.role === "user";
            const isErr = m.role === "error";
            return (
              <div
                key={m.id}
                className={cn(
                  "rounded-lg border p-4",
                  isUser
                    ? "border-cs-border bg-cs-card"
                    : isErr
                      ? "border-cs-danger/40 bg-cs-danger/10"
                      : "border-cs-border/60 bg-cs-card/60"
                )}
              >
                <div className="flex items-center gap-2 flex-wrap mb-2">
                  <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
                    {m.role}
                  </span>
                  {m.runtime && (
                    <span className={runtimeBadge(m.runtime)}>{m.runtime}</span>
                  )}
                  {m.agentSlug && (
                    <span className={personaBadge()}>
                      {personaDisplay(m.agentSlug)}
                    </span>
                  )}
                  <span className="ml-auto text-[11px] text-cs-muted">
                    {formatTime(m.createdAt)}
                  </span>
                </div>
                <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
                  {m.content}
                </pre>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
