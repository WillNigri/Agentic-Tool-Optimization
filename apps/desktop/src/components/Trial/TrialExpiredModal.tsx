import { useEffect, useState } from "react";
import { Clock, X, ExternalLink } from "lucide-react";

import { useTrialStatus } from "@/lib/tier";
import { UPGRADE_URL } from "@/lib/constants";
import { startCheckout, CheckoutError } from "@/lib/billing";
import { useAuthStore } from "@/hooks/useAuth";

// Phase 1 PR-A — trial-expired block modal.
//
// Renders only when the trial has expired AND the caller passes
// `open` (typically: the user just clicked a Pro-gated surface).
// Action-blocking, not launch-blocking — the war-room rejected
// "intercept on app start" because it punishes free users on every
// cold launch even when they're not trying to use a Pro feature.
//
// Wired in PR-B inside TierGate: when a Free user clicks a locked
// surface AND the trial is expired, this modal renders instead of
// UpgradePrompt (same `promptOpen` state — no separate store).

type Props = {
  open: boolean;
  onClose: () => void;
};

export default function TrialExpiredModal({ open, onClose }: Props) {
  const trial = useTrialStatus();
  const accessToken = useAuthStore((s) => s.accessToken);
  const [checkoutPending, setCheckoutPending] = useState(false);
  const [checkoutNotice, setCheckoutNotice] = useState<string | null>(null);
  // Clear stale notices when the modal closes so the user doesn't see
  // a previous attempt's error on the next reopen.
  useEffect(() => {
    if (!open) setCheckoutNotice(null);
  }, [open]);
  if (!open) return null;
  // Defensive: callers should only open this when expired, but if
  // they don't, render nothing rather than confuse the user.
  if (trial.state !== "expired") return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="trial-expired-title"
      className="fixed inset-0 z-[55] flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-md rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-start justify-between border-b border-cs-border p-5">
          <div className="flex items-start gap-3 min-w-0">
            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-cs-accent/10">
              <Clock size={16} className="text-cs-accent" aria-hidden />
            </div>
            <div className="min-w-0">
              <h2
                id="trial-expired-title"
                className="text-sm font-semibold text-cs-text"
              >
                Your Pro trial has ended
              </h2>
              <p className="text-[11px] text-cs-muted mt-0.5">
                Upgrade to keep using regression detection, cost benchmarks,
                evaluators, and the rest of Pro.
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="shrink-0 text-cs-muted hover:text-cs-text"
          >
            <X size={18} />
          </button>
        </header>

        <div className="p-5 text-xs text-cs-muted">
          Your local-only features keep working. Pro features become available
          again the moment you upgrade.
          {checkoutNotice && (
            <p
              role="alert"
              aria-live="polite"
              className="mt-3 text-[11px] text-cs-accent break-words"
            >
              {checkoutNotice}
            </p>
          )}
        </div>

        <footer className="flex items-center justify-between gap-3 px-5 pb-5">
          <button
            type="button"
            onClick={onClose}
            className="text-xs text-cs-muted hover:text-cs-text"
          >
            Not now
          </button>
          {accessToken ? (
            <button
              type="button"
              disabled={checkoutPending}
              aria-busy={checkoutPending}
              onClick={async () => {
                setCheckoutPending(true);
                setCheckoutNotice(null);
                try {
                  const result = await startCheckout("pro", accessToken);
                  if (result.kind === "calendly-fallback") {
                    setCheckoutNotice(result.notice);
                  }
                } catch (err) {
                  setCheckoutNotice(
                    err instanceof CheckoutError
                      ? `${err.message} (${err.code})`
                      : "Couldn't open checkout. Try again or use the onboarding link.",
                  );
                } finally {
                  setCheckoutPending(false);
                }
              }}
              className="inline-flex items-center gap-1.5 rounded-lg bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent/90 disabled:opacity-60"
            >
              {checkoutPending ? "Opening…" : "Upgrade"} <ExternalLink size={12} aria-hidden />
            </button>
          ) : (
            <a
              href={UPGRADE_URL}
              target="_blank"
              rel="noreferrer noopener"
              className="inline-flex items-center gap-1.5 rounded-lg bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent/90"
            >
              Upgrade <ExternalLink size={12} aria-hidden />
            </a>
          )}
        </footer>
      </div>
    </div>
  );
}
