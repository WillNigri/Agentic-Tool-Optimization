import { useState, useEffect, lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp, MessageSquare, Terminal as TerminalIcon, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useTerminalStore } from "@/stores/useTerminalStore";

// v1.3.0 — Embedded terminal pane (T5).
// Two modes (toggled in the header):
//   - Chat: existing PromptBar behavior wrapped here for consistency
//   - Shell: real interactive PTY via xterm.js + portable-pty (Rust)
// Persists collapsed/expanded + last tab + height to localStorage.

const PromptBar = lazy(() => import("@/components/PromptBar"));
const TerminalShellTab = lazy(() => import("./TerminalShellTab"));

type TabId = "chat" | "shell";

const STORAGE_TAB = "ato.terminal.tab.v1";
const STORAGE_OPEN = "ato.terminal.open.v1";

export default function TerminalPane() {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TabId>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_TAB);
      if (stored === "chat" || stored === "shell") return stored;
    } catch {
      // ignore
    }
    return "chat";
  });
  const [open, setOpen] = useState<boolean>(() => {
    try {
      return localStorage.getItem(STORAGE_OPEN) === "1";
    } catch {
      return false;
    }
  });

  // External "show me a shell" requests (e.g., from the Create Agent wizard's
  // Connect step). When the store fires a pending request, force the pane open
  // and switch to the Shell tab so the consumer can act on it.
  const pendingRequest = useTerminalStore((s) => s.pendingRequest);
  useEffect(() => {
    if (pendingRequest && pendingRequest.kind === "open-shell") {
      setTab("shell");
      setOpen(true);
    }
  }, [pendingRequest]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_TAB, tab);
    } catch {
      // ignore
    }
  }, [tab]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_OPEN, open ? "1" : "0");
    } catch {
      // ignore
    }
  }, [open]);

  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  return (
    <div className="border-t border-cs-border bg-cs-card">
      <header className="flex items-center justify-between px-3 py-1.5 gap-2">
        <div className="flex items-center gap-1">
          <PillButton
            active={tab === "chat"}
            onClick={() => {
              setTab("chat");
              setOpen(true);
            }}
            icon={<MessageSquare size={12} />}
          >
            {t("terminal.chat", "Chat")}
          </PillButton>
          <PillButton
            active={tab === "shell"}
            onClick={() => {
              setTab("shell");
              setOpen(true);
            }}
            icon={<TerminalIcon size={12} />}
          >
            {t("terminal.shell", "Shell")}
          </PillButton>
        </div>
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="flex items-center gap-1 text-xs text-cs-muted hover:text-cs-text"
          aria-label={open ? t("terminal.collapse", "Collapse") : t("terminal.expand", "Expand")}
        >
          {open ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
        </button>
      </header>

      {open && (
        <div className="border-t border-cs-border" style={{ height: 320 }}>
          {tab === "chat" ? (
            <Suspense
              fallback={
                <div className="flex items-center justify-center h-full">
                  <Loader2 size={16} className="animate-spin text-cs-muted" />
                </div>
              }
            >
              {/* PromptBar is self-contained; render as-is. */}
              <div className="h-full">
                <PromptBar />
              </div>
            </Suspense>
          ) : isTauri ? (
            <Suspense
              fallback={
                <div className="flex items-center justify-center h-full">
                  <Loader2 size={16} className="animate-spin text-cs-muted" />
                </div>
              }
            >
              <TerminalShellTab />
            </Suspense>
          ) : (
            <div className="flex items-center justify-center h-full text-xs text-cs-muted">
              {t("terminal.shellWebOnly", "Shell mode requires the desktop app.")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function PillButton({
  active,
  onClick,
  icon,
  children,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-xs font-medium transition",
        active
          ? "bg-cs-accent/15 text-cs-accent"
          : "text-cs-muted hover:text-cs-text hover:bg-cs-border/40"
      )}
    >
      {icon}
      {children}
    </button>
  );
}
