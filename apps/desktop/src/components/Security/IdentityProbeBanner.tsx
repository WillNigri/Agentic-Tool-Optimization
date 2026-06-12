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

// v2.15.6 — mirrors `rekey::RekeyApplicability`. The v1→v2 rekey
// flow only applies in the narrow state where v1 is still the
// active ledger version. After migration, the banner's "Re-key now"
// CTA points at a flow that can't help — so we suppress it.
type RekeyApplicability = {
  v1_active: boolean;
  v1_retired_at: string | null;
  active_version: string | null;
  applicable: boolean;
};

export default function IdentityProbeBanner() {
  const [status, setStatus] = useState<ProbeStatus | null>(null);
  const [applicability, setApplicability] = useState<RekeyApplicability | null>(null);
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

    // v2.15.6 — read ledger state once on mount. Used to suppress
    // the Re-key CTA when v1 has already been retired.
    void invoke<RekeyApplicability>("get_rekey_applicability")
      .then((a) => {
        if (!cancelled) setApplicability(a);
      })
      .catch(() => {
        // Command missing on older binaries → leave applicability null
        // → fall back to pre-v2.15.6 behaviour (always offer rekey).
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

  // v2.15.6 — when v1 is already retired, the v1→v2 rekey flow can't
  // help. Show a different message: explain the state + offer dismiss
  // only. Re-entering API keys is the recovery path for the actual
  // bug class behind a v2-era probe mismatch (cross-process keychain
  // ACL split, dev-build orphans, etc).
  const rekeyApplicable = applicability?.applicable ?? true;

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
              {rekeyApplicable
                ? "Master-key identity changed"
                : "Master-key already migrated"}
            </div>
            <div className="text-xs text-amber-100/80 mt-1">
              {rekeyApplicable ? (
                <>
                  The binary signing identity of ATO has changed since this
                  keychain was last used. Stored API keys may stop
                  decrypting on the next launch. Re-key now to migrate
                  them under the new identity.
                </>
              ) : (
                <>
                  Your install already migrated to{" "}
                  <code className="font-mono">
                    {applicability?.active_version ?? "v2"}
                  </code>
                  {applicability?.v1_retired_at && (
                    <>
                      {" "}
                      on{" "}
                      <code className="font-mono">
                        {applicability.v1_retired_at.slice(0, 10)}
                      </code>
                    </>
                  )}
                  . The v1→v2 rekey flow does not apply here. If saved API
                  keys aren't decrypting, the recovery path is{" "}
                  <strong>Settings → API Keys</strong> → re-enter the key
                  text (a future release will surface a v
                  {applicability?.active_version ?? "2"}→v
                  {applicability?.active_version === "v1" ? "2" : "next"}{" "}
                  rekey when it exists).
                </>
              )}
            </div>
            <div className="mt-3 flex items-center gap-2">
              {rekeyApplicable && (
                <button
                  onClick={() => setRekeyOpen(true)}
                  className="px-3 py-1.5 text-xs font-medium rounded bg-amber-500 hover:bg-amber-400 text-cs-bg"
                >
                  Re-key now
                </button>
              )}
              <button
                onClick={() => setDismissed(true)}
                className={
                  rekeyApplicable
                    ? "px-3 py-1.5 text-xs text-amber-100/70 hover:text-amber-100"
                    : "px-3 py-1.5 text-xs font-medium rounded bg-amber-500 hover:bg-amber-400 text-cs-bg"
                }
                title={
                  rekeyApplicable
                    ? "Dismiss until next app launch. Re-key still recommended."
                    : "Dismiss this notice."
                }
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
