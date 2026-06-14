/**
 * TetherApprovalModal — v2.17 Wave 2
 *
 * Shown when an incoming pair_request arrives from a browser session.
 * Listens for the `tether_approval_requested` Tauri event emitted by
 * tether_host.rs and renders a modal asking the user to Allow once,
 * Allow always, or Deny the browser's request to decrypt team shares.
 *
 * On decision, invokes the `tether_resolve_approval` Tauri command
 * which forwards the choice back to the host task.
 *
 * Mount at app root so it renders on top of any view. Multiple pending
 * approvals queue up; each is shown in FIFO order.
 */

import { useEffect, useRef, useState } from "react";
import { Shield, X } from "lucide-react";

// Tauri event/invoke APIs — dynamically imported so the component
// compiles in non-Tauri web-preview mode (though it never shows there).
async function tauriListen(
  event: string,
  handler: (payload: unknown) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen(event, (e) => handler(e.payload));
}

async function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke(cmd, args);
}

// ── Types (mirror tether_host.rs TetherApprovalRequested) ────────────────

interface ApprovalRequest {
  session_id: string;
  /** First 6 chars of the SHA-256 hash — shown as a session hint. */
  ua_hint: string;
  browser_ip_class: string | null;
  machine_name: string;
}

type Decision = "once" | "always" | "deny";

// ── Main component ────────────────────────────────────────────────────────

const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

export default function TetherApprovalModal() {
  // Queue of pending approval requests; shown one at a time.
  const [queue, setQueue] = useState<ApprovalRequest[]>([]);
  const [deciding, setDeciding] = useState(false);
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (!isTauri) return;

    // Subscribe to approval requests from the Rust host task.
    tauriListen("tether_approval_requested", (payload) => {
      const req = payload as ApprovalRequest;
      setQueue((q) => [...q, req]);
    }).then((unlisten) => {
      unlistenRef.current = unlisten;
    });

    return () => {
      unlistenRef.current?.();
    };
  }, []);

  const current = queue[0];
  if (!current) return null;

  async function decide(d: Decision) {
    if (deciding) return;
    setDeciding(true);
    try {
      await tauriInvoke("tether_resolve_approval", {
        decision: { session_id: current.session_id, decision: d },
      });
    } catch (err) {
      console.error("[TetherApprovalModal] tether_resolve_approval failed:", err);
    } finally {
      // Remove the current request from the queue regardless of outcome.
      setQueue((q) => q.slice(1));
      setDeciding(false);
    }
  }

  return (
    // Full-screen overlay with z-[999] to sit above all other modals.
    <div
      className="fixed inset-0 z-[999] flex items-center justify-center bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="Browser tether approval"
    >
      <div className="relative w-full max-w-sm rounded-xl border border-white/10 bg-[#0f0f1a] p-6 shadow-2xl">
        {/* Header */}
        <div className="mb-4 flex items-start gap-3">
          <div className="mt-0.5 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-cyan-400/10">
            <Shield className="h-5 w-5 text-cyan-400" />
          </div>
          <div>
            <h2 className="text-sm font-semibold text-white">Decrypt team shares?</h2>
            <p className="mt-0.5 text-xs text-white/50">on {current.machine_name}</p>
          </div>
          {/* Dismiss (= Deny) via X */}
          <button
            onClick={() => void decide("deny")}
            className="ml-auto text-white/30 transition hover:text-white/70"
            aria-label="Deny and close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Body */}
        <p className="mb-5 text-sm leading-relaxed text-white/70">
          A browser session{" "}
          <span className="font-mono text-white/90">{current.ua_hint}…</span>
          {current.browser_ip_class ? ` (${current.browser_ip_class})` : ""} wants to
          decrypt E2E-encrypted team shares on this Mac using the Team Key cached in memory.
        </p>

        {/* Action buttons */}
        <div className="flex flex-col gap-2">
          <button
            onClick={() => void decide("once")}
            disabled={deciding}
            className="w-full rounded-lg bg-cyan-500 py-2 text-sm font-medium text-black transition hover:bg-cyan-400 disabled:opacity-50"
          >
            Allow once
          </button>
          <button
            onClick={() => void decide("always")}
            disabled={deciding}
            className="w-full rounded-lg border border-cyan-500/40 bg-cyan-500/10 py-2 text-sm font-medium text-cyan-300 transition hover:bg-cyan-500/20 disabled:opacity-50"
          >
            Allow always for this browser
          </button>
          <button
            onClick={() => void decide("deny")}
            disabled={deciding}
            className="w-full rounded-lg border border-white/10 py-2 text-sm font-medium text-white/50 transition hover:border-white/20 hover:text-white/70 disabled:opacity-50"
          >
            Deny
          </button>
        </div>

        {/* Queue indicator */}
        {queue.length > 1 && (
          <p className="mt-3 text-center text-xs text-white/30">
            {queue.length - 1} more request{queue.length > 2 ? "s" : ""} waiting
          </p>
        )}
      </div>
    </div>
  );
}
