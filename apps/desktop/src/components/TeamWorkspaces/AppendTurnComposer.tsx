// AppendTurnComposer — bottom-of-panel textarea + Send for live shares.
//
// Plaintext shares: fully enabled; sends a `turn_appended` event with
//   { role: 'user', text: <value> } via appendTeamEvent.
// E2E shares: composer is rendered but disabled; a tooltip explains
//   that encrypted append ships in Wave 3.
//
// On success the textarea clears and the event arrives back via the WS
// stream (useTeamEventStream), which renders it in the Live section —
// no optimistic UI needed.

import { useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Send, Lock } from "lucide-react";
import { cn } from "@/lib/utils";
import { appendTeamEvent } from "@/lib/cloud-api";
import type { SharedResourceKind } from "@/lib/cloud-api";

interface AppendTurnComposerProps {
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  /** If true the share is E2E-encrypted; composer is disabled until Wave 3. */
  isE2e: boolean;
}

export default function AppendTurnComposer({
  teamId,
  kind,
  resourceId,
  isE2e,
}: AppendTurnComposerProps) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const disabled = isE2e || isSending;

  async function handleSend() {
    const trimmed = text.trim();
    if (!trimmed || disabled) return;

    setIsSending(true);
    setSendError(null);
    try {
      await appendTeamEvent(teamId, kind, resourceId, {
        event_kind: "turn_appended",
        payload_json: { role: "user", text: trimmed },
        surface: "desktop",
      });
      setText("");
      textareaRef.current?.focus();
    } catch (err) {
      setSendError(
        err instanceof Error
          ? err.message
          : t("teamShare.composer.sendError", {
              defaultValue: "Failed to send. Try again.",
            }),
      );
    } finally {
      setIsSending(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    // ⌘Enter / Ctrl+Enter sends; plain Enter adds a newline.
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void handleSend();
    }
  }

  return (
    <div className="mt-4 space-y-2">
      {isE2e && (
        <div
          className={cn(
            "flex items-center gap-2 rounded-md border border-cs-accent/30 bg-cs-accent/5",
            "px-3 py-2 text-[11px] text-cs-muted",
          )}
          title={t("teamShare.composer.e2eTooltip", {
            defaultValue:
              "Encrypted append shipping in Wave 3. Plaintext append is disabled for E2E shares.",
          })}
        >
          <Lock size={11} className="shrink-0 text-cs-accent" />
          {t("teamShare.composer.e2eHint", {
            defaultValue: "Encrypted append shipping in Wave 3.",
          })}
        </div>
      )}
      <div className="flex gap-2">
        <textarea
          ref={textareaRef}
          data-testid="append-turn-textarea"
          rows={2}
          disabled={disabled}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setSendError(null);
          }}
          onKeyDown={handleKeyDown}
          placeholder={
            isE2e
              ? t("teamShare.composer.placeholderE2e", {
                  defaultValue: "Encrypted append ships in Wave 3…",
                })
              : t("teamShare.composer.placeholder", {
                  defaultValue: "Add a turn… (⌘↵ to send)",
                })
          }
          className={cn(
            "flex-1 resize-none rounded-md border bg-cs-bg-raised",
            "px-3 py-2 text-xs text-cs-text placeholder:text-cs-muted",
            "focus:outline-none focus:ring-1 focus:ring-cs-accent/60",
            disabled
              ? "cursor-not-allowed border-cs-border/30 opacity-50"
              : "border-cs-border/60",
          )}
        />
        <button
          onClick={() => void handleSend()}
          disabled={disabled || !text.trim()}
          aria-label={t("teamShare.composer.sendLabel", { defaultValue: "Send turn" })}
          className={cn(
            "flex items-center justify-center rounded-md border px-3 py-2",
            "text-xs transition-colors",
            disabled || !text.trim()
              ? "cursor-not-allowed border-cs-border/30 text-cs-muted opacity-50"
              : "border-cs-accent/60 bg-cs-accent/10 text-cs-accent hover:bg-cs-accent/20",
          )}
        >
          {isSending ? (
            <span className="animate-pulse">…</span>
          ) : (
            <Send size={12} />
          )}
        </button>
      </div>
      {sendError && (
        <p className="text-[11px] text-cs-danger">{sendError}</p>
      )}
    </div>
  );
}
