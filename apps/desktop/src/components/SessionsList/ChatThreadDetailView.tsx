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
//
// v2.7.13 — close lifecycle parity. The Close button + summary card
// mirror SessionTranscriptView's surface; the modal is the shared
// CloseConversationModal with conversationType="chat".

import { useState } from "react";
import { Loader2, Lock, Sparkles, Tag as TagIcon, Unlock } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import { buildChatSnapshot } from "@/lib/teamShareSnapshot";
import {
  runtimeBadge,
  personaBadge,
  personaDisplay,
  formatTime,
} from "./_helpers";
import CloseConversationModal from "./CloseConversationModal";
import ShareWithTeamButton from "@/components/TeamWorkspaces/ShareWithTeamButton";

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

// chat_threads row snapshot returned by `get_chat`. Maps directly
// to commands::chats::ChatThread on the Rust side. v2.7.14: serde
// rename_all = "camelCase" is set there now so the wire shape
// matches every other Tauri command's response.
interface ChatThreadSnapshot {
  id: string;
  title: string;
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  coordinatorRuntime: string | null;
  humanComment: string | null;
  tags: string[];
  messageCount: number;
}

export default function ChatThreadDetailView({
  threadId,
  onBack,
}: {
  threadId: string;
  onBack: () => void;
}) {
  const qc = useQueryClient();
  const q = useQuery<ChatMessage[]>({
    queryKey: ["chat-messages", threadId],
    queryFn: () => invoke<ChatMessage[]>("get_chat_messages", { threadId }),
    staleTime: 30_000,
  });
  // v2.7.13 — chat_threads row snapshot for the close lifecycle.
  // Independent of get_chat_messages so a slow messages query doesn't
  // block the snapshot (and vice versa).
  const snapshotQ = useQuery<ChatThreadSnapshot>({
    queryKey: ["chat-snapshot", threadId],
    queryFn: () => invoke<ChatThreadSnapshot>("get_chat", { chatId: threadId }),
    staleTime: 30_000,
    retry: false,
  });
  const isClosed = snapshotQ.data?.status === "closed";

  const [closing, setClosing] = useState(false);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [reopening, setReopening] = useState(false);
  const [reopenError, setReopenError] = useState<string | null>(null);
  const [closeModalOpen, setCloseModalOpen] = useState(false);

  const handleClose = async (opts: {
    coordinator: string | null;
    humanComment: string | null;
  }) => {
    if (closing) return;
    setCloseModalOpen(false);
    setClosing(true);
    setCloseError(null);
    setReopenError(null);
    try {
      await invoke("close_chat", {
        chatId: threadId,
        agentSlug: null,
        model: null,
        coordinator: opts.coordinator,
        humanComment: opts.humanComment,
      });
      await qc.invalidateQueries({ queryKey: ["chat-snapshot", threadId] });
      await qc.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      const msg = String(e);
      if (!msg.includes("__cancelled__")) {
        setCloseError(msg);
      }
    } finally {
      setClosing(false);
    }
  };

  const handleReopen = async () => {
    if (reopening) return;
    setReopening(true);
    setReopenError(null);
    setCloseError(null);
    try {
      await invoke("reopen_chat", { chatId: threadId });
      await qc.invalidateQueries({ queryKey: ["chat-snapshot", threadId] });
      await qc.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      setReopenError(String(e));
    } finally {
      setReopening(false);
    }
  };

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
        <div className="flex items-center gap-2">
          <ShareWithTeamButton
            resourceKind="chat"
            resourceId={threadId}
            getSnapshot={() => buildChatSnapshot(threadId)}
          />
          <div className="text-xs text-cs-muted font-mono">{threadId}</div>
          {isClosed ? (
            <button
              onClick={handleReopen}
              disabled={reopening}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Reopen this chat thread. The next close will refresh the summary with any newly-added messages."
            >
              {reopening ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Unlock size={14} />
              )}
              {reopening ? "Reopening…" : "Reopen"}
            </button>
          ) : (
            <button
              onClick={() => setCloseModalOpen(true)}
              disabled={closing || messages.length === 0}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Close this chat thread. You'll pick the coordinator LLM and can attach a note before the messages are summarized."
            >
              <Lock size={14} /> Close chat
            </button>
          )}
        </div>
      </div>

      {/* v2.7.13 — coordinator summary card at the TOP (matches the
          session + war-room layout — Will dogfood 2026-05-21: summary
          should always render above the conversation body). */}
      {isClosed && snapshotQ.data?.summary && (
        <div className="border border-cs-accent/30 rounded-md bg-cs-accent/5 p-3 space-y-2">
          <div className="text-xs font-medium uppercase text-cs-accent flex items-center gap-2">
            <Sparkles size={12} /> Coordinator summary
            {snapshotQ.data.closedAt && (
              <span className="text-[10px] text-cs-muted normal-case font-normal">
                · closed {formatTime(snapshotQ.data.closedAt)}
              </span>
            )}
            {snapshotQ.data.coordinatorRuntime && (
              <span className={cn(runtimeBadge(snapshotQ.data.coordinatorRuntime), "normal-case")}>
                {snapshotQ.data.coordinatorRuntime}
              </span>
            )}
          </div>
          {snapshotQ.data.autoTitle && (
            <div className="text-sm font-medium text-cs-text">
              {snapshotQ.data.autoTitle}
            </div>
          )}
          <div className="text-sm text-cs-text whitespace-pre-wrap">
            {snapshotQ.data.summary}
          </div>
          {snapshotQ.data.humanComment && snapshotQ.data.humanComment.trim() && (
            <div className="border-t border-cs-accent/20 pt-2 mt-2">
              <div className="text-[10px] uppercase tracking-wider font-medium text-cs-muted mb-1">
                Note from human
              </div>
              <div className="text-sm text-cs-text whitespace-pre-wrap">
                {snapshotQ.data.humanComment}
              </div>
            </div>
          )}
          {snapshotQ.data.tags.length > 0 && (
            <div className="flex items-center gap-1 flex-wrap pt-1">
              <TagIcon size={10} className="text-cs-muted" />
              {snapshotQ.data.tags.map((tag) => (
                <span
                  key={tag}
                  className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      )}

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

      {closeError && (
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <strong className="text-cs-danger">Close failed:</strong> {closeError}
        </div>
      )}
      {reopenError && (
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <strong className="text-cs-danger">Reopen failed:</strong>{" "}
          {reopenError}
        </div>
      )}

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

      {closing && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-cs-bg/80 backdrop-blur-sm">
          <div className="border border-cs-border bg-cs-card rounded-lg p-6 max-w-md w-full mx-4 space-y-4">
            <div className="flex items-center gap-3">
              <Loader2
                size={20}
                className="animate-spin text-cs-accent shrink-0"
              />
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium text-cs-text">
                  Coordinator is summarizing the chat…
                </div>
                <div className="text-xs text-cs-muted mt-1">
                  Reading every message. Typically finishes in 5–20 seconds.
                </div>
              </div>
            </div>
            <button
              type="button"
              onClick={() =>
                void invoke("cancel_close_session", {
                  sessionId: threadId,
                }).catch(() => undefined)
              }
              className="w-full px-3 py-1.5 rounded-md border border-cs-border bg-cs-card text-sm text-cs-muted hover:text-cs-text"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <CloseConversationModal
        open={closeModalOpen}
        busy={closing}
        conversationType="chat"
        onCancel={() => setCloseModalOpen(false)}
        onSubmit={handleClose}
      />
    </div>
  );
}
