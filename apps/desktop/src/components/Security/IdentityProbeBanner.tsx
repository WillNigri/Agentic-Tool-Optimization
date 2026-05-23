// Identity-probe mismatch banner — PR-5 of master_key_v2.
//
// Renders ONLY when the backend's PR-3 (`run_full_probe_cycle`)
// returned `Mismatched`. Surfaces "the binary signing identity
// changed; your stored API keys may need re-entry" with a Rekey
// button that opens `RekeyMasterKeyModal`.
//
// Wiring per PR-3's contract:
//   - subscribe to `identity-probe-status` Tauri event on mount
//     (fired once from lib.rs::run's .setup with the startup status)
//   - also call `get_identity_probe_status` on mount to handle the
//     race where the React component mounts AFTER the event fires
//   - `Unknown.reason` is OPS-FACING ONLY per PR-3's locked
//     contract — never render verbatim; show generic "status check
//     unavailable" instead. Confirmed in identity_probe.rs docstring
//     "PR-5's UI MUST NOT render reason verbatim."

import { useEffect, useState, lazy, Suspense } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { AlertTriangle, X } from "lucide-react";

const RekeyMasterKeyModal = lazy(
  () => import("./RekeyMasterKeyModal"),
);

// Mirrors `identity_probe::ProbeStatus` — see PR-3's serde tag /
// rename_all contract test for the wire shape.
type ProbeStatus =
  | { status: "not_populated" }
  | { status: "matched" }
  | {
      status: "mismatched";
      stored_probe: string;
      computed_probe: string;
      detected_at: string;
      audit_logged: boolean;
    }
  | { status: "unknown"; reason: string };

export default function IdentityProbeBanner() {
  const [status, setStatus] = useState<ProbeStatus | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [rekeyOpen, setRekeyOpen] = useState(false);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;

    // Race-safe initial fetch: the event from .setup may have fired
    // before this component mounted, so we also poll the command.
    void invoke<ProbeStatus>("get_identity_probe_status")
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch(() => {
        // Tauri command unavailable (likely web-mode preview) —
        // silently keep status null so the banner stays hidden.
      });

    // Subscribe to future re-emits (e.g. after a successful rekey
    // the backend re-runs the probe cycle and updates state).
    void listen<ProbeStatus>("identity-probe-status", (event) => {
      if (!cancelled) setStatus(event.payload);
    }).then((un) => {
      if (cancelled) un();
      else unlisten = un;
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // Only `mismatched` surfaces. Everything else is silent.
  if (!status || status.status !== "mismatched" || dismissed) return null;

  return (
    <>
      <div
        role="alert"
        aria-live="polite"
        className="rounded-md border border-amber-500/40 bg-amber-500/10 px-4 py-3 shadow-lg"
      >
        <div className="flex items-start gap-3">
          <AlertTriangle
            className="text-amber-400 mt-0.5 flex-shrink-0"
            size={18}
          />
          <div className="flex-1 min-w-0">
            <div className="font-medium text-amber-100 text-sm">
              Master-key identity changed
            </div>
            <div className="text-xs text-amber-100/80 mt-1">
              The binary signing identity of ATO has changed since this
              keychain was last used. Stored API keys may stop
              decrypting on the next launch. Re-key now to migrate
              them under the new identity.
            </div>
            <div className="mt-3 flex items-center gap-2">
              <button
                onClick={() => setRekeyOpen(true)}
                className="px-3 py-1.5 text-xs font-medium rounded bg-amber-500 hover:bg-amber-400 text-cs-bg"
              >
                Re-key now
              </button>
              <button
                onClick={() => setDismissed(true)}
                className="px-3 py-1.5 text-xs text-amber-100/70 hover:text-amber-100"
                title="Dismiss until next app launch. Re-key still recommended."
              >
                Dismiss
              </button>
            </div>
          </div>
          <button
            onClick={() => setDismissed(true)}
            aria-label="Close"
            className="text-amber-100/60 hover:text-amber-100"
          >
            <X size={14} />
          </button>
        </div>
      </div>
      {rekeyOpen && (
        <Suspense fallback={null}>
          <RekeyMasterKeyModal
            onClose={() => setRekeyOpen(false)}
            onSuccess={() => {
              setRekeyOpen(false);
              // Backend re-runs the probe cycle on success + emits
              // a fresh identity-probe-status event; our listener
              // picks it up and the banner disappears.
            }}
          />
        </Suspense>
      )}
    </>
  );
}
