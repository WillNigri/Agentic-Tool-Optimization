// PromptBar/ThreadHistoryHeader.tsx — thread-history picker + project
// pill + "+" button. Sits above the chat-history and the input form.
//
// Extracted from PromptBar/index.tsx 2026-05-19 (v2.7.7 frontend
// elegance push). 160 lines of JSX moved out of the orchestrator.
//
// The thread dropdown is the WhatsApp-style fast-pickup list shipped
// in 688225a (2026-05-19 truncation war-room — hard cap at last 5,
// sorted by lastMessageAt desc, footer routes to Sessions tab when
// total > 5).
//
// The "+" button calls back into the orchestrator's `newThread`,
// which is now lazy (34f86d5) — it clears currentThreadId without
// writing an empty row. The actual `chat_threads` insert happens on
// first message dispatch.

import { ChevronDown, FolderKanban, History, MessageSquarePlus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import type { ChatThread } from "@/lib/chatThreads";

import { formatThreadAge } from "./_helpers";

interface RenamingThread {
  id: string;
  title: string;
}

interface ActiveProject {
  name: string;
}

interface Props {
  threads: ChatThread[];
  currentThread: ChatThread | null | undefined;
  currentThreadId: string | null;
  setCurrentThreadId: (id: string | null) => void;
  renamingThread: RenamingThread | null;
  setRenamingThread: (next: RenamingThread | null) => void;
  commitRename: () => Promise<void>;
  removeThread: (id: string) => Promise<void>;
  open: boolean;
  setOpen: (next: boolean | ((v: boolean) => boolean)) => void;
  setExpanded: (v: boolean) => void;
  newThread: () => Promise<void>;
  activeProject: ActiveProject | null;
  /** Routes to Runs → Sessions when the "See all N" footer is
   *  clicked. Orchestrator owns the setSection + setSubTab pair so
   *  this picker stays a layout-leaf with no Zustand coupling. */
  onSeeAll: () => void;
}

export function ThreadHistoryHeader({
  threads,
  currentThread,
  currentThreadId,
  setCurrentThreadId,
  renamingThread,
  setRenamingThread,
  commitRename,
  removeThread,
  open,
  setOpen,
  setExpanded,
  newThread,
  activeProject,
  onSeeAll,
}: Props) {
  const { t } = useTranslation();

  const sorted = [...threads].sort((a, b) => {
    const aTs = a.lastMessageAt ? new Date(a.lastMessageAt).getTime() : 0;
    const bTs = b.lastMessageAt ? new Date(b.lastMessageAt).getTime() : 0;
    return bTs - aTs;
  });
  const capped = sorted.slice(0, 5);
  const remaining = sorted.length - capped.length;

  return (
    <header className="shrink-0 flex items-center gap-2 px-3 py-1.5 border-b border-cs-border/60 bg-cs-bg-raised/40">
      <div className="relative shrink-0">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] text-cs-text hover:bg-cs-border/40"
        >
          <History size={12} className="text-cs-muted" />
          <span className="font-medium truncate max-w-[180px]">
            {currentThread?.title ?? t("prompt.noThread", "(new conversation)")}
          </span>
          <ChevronDown size={10} className="text-cs-muted" />
        </button>

        {open && (
          <>
            <div
              className="fixed inset-0 z-30"
              onClick={() => setOpen(false)}
            />
            <div className="absolute top-full left-0 mt-1 w-80 max-h-80 overflow-y-auto rounded-lg border border-cs-border bg-cs-card shadow-xl z-40">
              {threads.length === 0 ? (
                <p className="px-3 py-3 text-[11px] text-cs-muted">
                  {t("prompt.noThreads", "No conversations yet.")}
                </p>
              ) : (
                <>
                  {capped.map((thr) => {
                    const isCurrent = thr.id === currentThreadId;
                    const isRenaming = renamingThread?.id === thr.id;
                    return (
                      <div
                        key={thr.id}
                        className={cn(
                          "group flex items-center gap-2 px-3 py-1.5 transition-colors",
                          isCurrent ? "bg-cs-accent/5" : "hover:bg-cs-bg",
                        )}
                      >
                        {isRenaming ? (
                          <input
                            type="text"
                            value={renamingThread.title}
                            onChange={(e) =>
                              setRenamingThread({
                                id: thr.id,
                                title: e.target.value,
                              })
                            }
                            onKeyDown={(e) => {
                              if (e.key === "Enter") void commitRename();
                              if (e.key === "Escape") setRenamingThread(null);
                            }}
                            onBlur={() => void commitRename()}
                            autoFocus
                            className="flex-1 bg-cs-bg border border-cs-accent/40 rounded px-2 py-0.5 text-xs text-cs-text focus:outline-none"
                          />
                        ) : (
                          <button
                            type="button"
                            onClick={() => {
                              setCurrentThreadId(thr.id);
                              setOpen(false);
                              setExpanded(true);
                            }}
                            onDoubleClick={() =>
                              setRenamingThread({
                                id: thr.id,
                                title: thr.title,
                              })
                            }
                            title={`${thr.title} · ${thr.messageCount} msgs`}
                            className="flex-1 min-w-0 text-left text-xs flex items-center gap-2"
                          >
                            <span
                              className={cn(
                                "truncate flex-1 font-medium",
                                isCurrent ? "text-cs-accent" : "text-cs-text",
                              )}
                            >
                              {thr.title}
                            </span>
                            <span className="text-[10px] text-cs-muted shrink-0">
                              {formatThreadAge(thr.lastMessageAt)}
                            </span>
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            void removeThread(thr.id);
                          }}
                          className="opacity-0 group-hover:opacity-100 text-cs-muted hover:text-cs-danger shrink-0 p-1"
                          aria-label={t("common.delete", "Delete")}
                        >
                          <Trash2 size={10} />
                        </button>
                      </div>
                    );
                  })}
                  {remaining > 0 && (
                    <button
                      type="button"
                      onClick={() => {
                        setOpen(false);
                        onSeeAll();
                      }}
                      className="w-full border-t border-cs-border px-3 py-2 text-[11px] text-cs-muted hover:text-cs-accent hover:bg-cs-bg flex items-center justify-center gap-1"
                    >
                      {t("prompt.seeAllThreads", {
                        count: threads.length,
                        defaultValue: "See all {{count}} conversations →",
                      })}
                    </button>
                  )}
                </>
              )}
            </div>
          </>
        )}
      </div>

      <div className="flex-1" />

      {activeProject && (
        <span className="inline-flex items-center gap-1 rounded-md bg-cs-bg px-2 py-0.5 text-[10px] text-cs-muted">
          <FolderKanban size={10} />
          {activeProject.name}
        </span>
      )}

      <button
        type="button"
        onClick={newThread}
        className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-[10px] text-cs-muted hover:text-cs-accent"
        title={t("prompt.newThreadTitle", "New conversation")}
      >
        <MessageSquarePlus size={11} />
      </button>
    </header>
  );
}
