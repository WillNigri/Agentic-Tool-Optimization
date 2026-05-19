// PromptBar/RoomTypePicker.tsx — bottom-pane room-type launcher.
//
// 2026-05-19 — war-room E5EB8C87-…FB34 (claude + codex unanimous):
// Path B's chevron-buried launcher was a dead affordance, so this
// dropdown moves to the input row where users already look for
// runtime / agent. Owns its own popover state; the orchestrator
// hands in the three launch callbacks.
//
// `_helpers.ts` is explicitly no-JSX / no-React-state, so this lives
// next to ChatRow.tsx, not in helpers.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { MessageSquare, MessageSquarePlus, Swords } from "lucide-react";

interface Props {
  /** Quick chat — stay in this pane, no modal. Orchestrator focuses
   *  the input so the user can type immediately. */
  onQuickChat: () => void;
  /** Multi-turn session — opens NewSessionModal (orchestrator routes
   *  to Sessions tab + sets pendingOpenNewSession). */
  onNewSession: () => void;
  /** War room — opens FirstChatWizard (orchestrator flips
   *  firstChatOpen in Zustand; wizard is globally mounted from
   *  Dashboard since v2.7.6). */
  onWarRoom: () => void;
}

export function RoomTypePicker({ onQuickChat, onNewSession, onWarRoom }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const choose = (action: () => void) => {
    setOpen(false);
    action();
  };

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        data-demo-id="room-type-picker"
        className="flex items-center gap-1 px-2 py-1.5 rounded-lg border border-cs-accent/30 bg-cs-accent/5 text-cs-accent hover:border-cs-accent/60 transition-colors"
        title={t(
          "prompt.roomTypeTitle",
          "Choose what kind of conversation to start"
        )}
      >
        <MessageSquarePlus size={12} />
        <span className="text-[10px] font-medium">
          {t("prompt.roomTypeNew", "New ▾")}
        </span>
      </button>

      {open && (
        <>
          <div
            className="fixed inset-0 z-30"
            onClick={() => setOpen(false)}
          />
          <div className="absolute bottom-full left-0 mb-1 w-56 rounded-lg border border-cs-border bg-cs-card shadow-xl z-40 overflow-hidden">
            <div className="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wider text-cs-muted">
              {t("prompt.startNew", "Start new")}
            </div>
            <button
              type="button"
              onClick={() => choose(onQuickChat)}
              className="w-full flex items-center gap-2 px-3 py-2 text-xs hover:bg-cs-border/40"
              title={t(
                "prompt.newQuickChatTitle",
                "One-on-one chat in this pane. Uses the runtime + agent on the right."
              )}
            >
              <MessageSquarePlus size={12} className="text-cs-accent" />
              <span className="flex-1 text-left text-cs-text">
                🗨 {t("prompt.newQuickChat", "Quick chat")}
              </span>
              <span className="text-[10px] text-cs-muted">here</span>
            </button>
            <button
              type="button"
              onClick={() => choose(onNewSession)}
              className="w-full flex items-center gap-2 px-3 py-2 text-xs hover:bg-cs-border/40"
              title={t(
                "prompt.newSessionTitle",
                "Multi-turn session with lifecycle (open / close / coordinator summary)."
              )}
            >
              <MessageSquare size={12} className="text-cs-muted" />
              <span className="flex-1 text-left text-cs-text">
                💬 {t("prompt.newSession", "Multi-turn session")}
              </span>
              <span className="text-[10px] text-cs-muted">Sessions tab</span>
            </button>
            <button
              type="button"
              onClick={() => choose(onWarRoom)}
              className="w-full flex items-center gap-2 px-3 py-2 text-xs hover:bg-cs-border/40"
              title={t(
                "prompt.newWarRoomTitle",
                "Fire the same prompt to every connected LLM in parallel."
              )}
            >
              <Swords size={12} className="text-cs-accent" />
              <span className="flex-1 text-left text-cs-text">
                ⚔ {t("prompt.newWarRoom", "War room")}
              </span>
              <span className="text-[10px] text-cs-muted">wizard</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
}
