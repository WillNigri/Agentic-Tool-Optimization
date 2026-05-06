import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { Loader2, AlertCircle, RefreshCcw } from "lucide-react";
import { spawnPty, type PtyHandle } from "@/lib/pty";
import { useProjectStore } from "@/stores/useProjectStore";
import { useTerminalStore } from "@/stores/useTerminalStore";

// v1.3.0 — Real interactive shell tab inside the embedded terminal pane (T5).
// xterm.js + Tauri PTY. Scopes CWD to the active project when one is set.

export default function TerminalShellTab() {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const ptyRef = useRef<PtyHandle | null>(null);
  const [status, setStatus] = useState<"booting" | "ready" | "error" | "exited">("booting");
  const [error, setError] = useState<string | null>(null);
  const [restartKey, setRestartKey] = useState(0);
  const activeProject = useProjectStore((s) => s.activeProject);
  const pendingRequest = useTerminalStore((s) => s.pendingRequest);
  const clearRequest = useTerminalStore((s) => s.clearRequest);

  useEffect(() => {
    let cancelled = false;
    const el = containerRef.current;
    if (!el) return;

    const term = new Terminal({
      fontSize: 12,
      fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
      cursorBlink: true,
      scrollback: 5000,
      theme: {
        background: "#0a0a0f",
        foreground: "#e8e8f0",
        cursor: "#00FFB2",
        black: "#16161e",
        green: "#00FFB2",
        yellow: "#FFB800",
        red: "#FF4466",
      },
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());
    term.open(el);
    fitAddon.fit();
    termRef.current = term;
    fitRef.current = fitAddon;

    const dims = () => ({ rows: term.rows, cols: term.cols });

    (async () => {
      try {
        const handle = await spawnPty({
          cwd: activeProject?.path,
          rows: dims().rows,
          cols: dims().cols,
          onData: (data) => {
            term.write(data);
          },
          onExit: (code) => {
            if (cancelled) return;
            term.writeln(`\r\n\x1b[2m[process exited${code !== null ? ` with code ${code}` : ""}]\x1b[0m`);
            setStatus("exited");
          },
        });
        if (cancelled) {
          await handle.kill();
          return;
        }
        ptyRef.current = handle;
        setStatus("ready");

        // Wire xterm input → PTY stdin
        term.onData((d) => {
          handle.write(d).catch(() => {});
        });

        // If we entered the shell because of a pending external request, run
        // the requested initial command now that the PTY is ready.
        if (pendingRequest && pendingRequest.kind === "open-shell" && pendingRequest.initialCommand) {
          // Short delay so the user sees the shell prompt land before we type.
          setTimeout(() => {
            handle.write(pendingRequest.initialCommand + "\n").catch(() => {});
            // Optional follow-up keys (e.g., "@<slug> ") sent without newline
            // so the user can finish their thought. Wait long enough for
            // claude/codex/gemini to print their prompt.
            if (pendingRequest.followUpKeys) {
              setTimeout(() => {
                handle.write(pendingRequest.followUpKeys ?? "").catch(() => {});
                clearRequest();
              }, pendingRequest.followUpDelayMs ?? 1500);
            } else {
              clearRequest();
            }
          }, 250);
        }
      } catch (err) {
        if (cancelled) return;
        setStatus("error");
        setError(err instanceof Error ? err.message : String(err));
      }
    })();

    // Resize observer for the container.
    const ro = new ResizeObserver(() => {
      try {
        fitAddon.fit();
        const { rows, cols } = dims();
        if (ptyRef.current) {
          ptyRef.current.resize(rows, cols).catch(() => {});
        }
      } catch {
        // ignore mid-mount races
      }
    });
    ro.observe(el);

    return () => {
      cancelled = true;
      ro.disconnect();
      const handle = ptyRef.current;
      ptyRef.current = null;
      if (handle) {
        handle.kill().catch(() => {});
      }
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  // restart on key change so Reconnect button can re-spawn
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [restartKey, activeProject?.path]);

  // If a request comes in *after* the shell is already running, type the
  // command into the existing PTY (don't re-spawn).
  useEffect(() => {
    if (!pendingRequest || pendingRequest.kind !== "open-shell" || !pendingRequest.initialCommand) {
      return;
    }
    const handle = ptyRef.current;
    if (!handle || status !== "ready") return;
    handle.write(pendingRequest.initialCommand + "\n").catch(() => {});
    if (pendingRequest.followUpKeys) {
      setTimeout(() => {
        handle.write(pendingRequest.followUpKeys ?? "").catch(() => {});
        clearRequest();
      }, pendingRequest.followUpDelayMs ?? 1500);
    } else {
      clearRequest();
    }
  }, [pendingRequest, status, clearRequest]);

  return (
    <div className="relative h-full flex flex-col bg-[#0a0a0f]">
      <div
        ref={containerRef}
        className="flex-1 min-h-0 overflow-hidden p-1.5"
        aria-label={t("terminal.shellLabel", "Interactive shell")}
      />
      {status === "booting" && (
        <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
          <span className="inline-flex items-center gap-2 text-xs text-cs-muted bg-cs-bg/80 rounded-md px-2 py-1">
            <Loader2 size={12} className="animate-spin" />
            {t("terminal.starting", "Starting shell…")}
          </span>
        </div>
      )}
      {status === "error" && (
        <div className="absolute inset-0 flex items-center justify-center p-6">
          <div className="flex items-start gap-3 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-4 max-w-md">
            <AlertCircle size={16} className="text-cs-danger shrink-0 mt-0.5" />
            <div className="flex-1">
              <p className="text-xs text-cs-text">
                {t("terminal.spawnError", "Couldn't start the shell.")}
              </p>
              {error && <p className="mt-1 text-[11px] font-mono text-cs-muted">{error}</p>}
              <button
                type="button"
                onClick={() => {
                  setError(null);
                  setStatus("booting");
                  setRestartKey((k) => k + 1);
                }}
                className="mt-2 inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
              >
                <RefreshCcw size={10} />
                {t("terminal.retry", "Retry")}
              </button>
            </div>
          </div>
        </div>
      )}
      {status === "exited" && (
        <button
          type="button"
          onClick={() => {
            setStatus("booting");
            setRestartKey((k) => k + 1);
          }}
          className="absolute right-2 top-2 inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-2 py-1 text-[11px] text-cs-text hover:border-cs-hover"
        >
          <RefreshCcw size={10} />
          {t("terminal.restart", "Restart")}
        </button>
      )}
    </div>
  );
}
