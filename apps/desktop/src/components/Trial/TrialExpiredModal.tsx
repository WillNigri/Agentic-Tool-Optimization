import { Clock, X, ExternalLink } from "lucide-react";

import { useTrialStatus } from "@/lib/tier";

// Phase 1 PR-A — trial-expired block modal.
//
// Renders only when the trial has expired AND the caller passes
// `open` (typically: the user just clicked a Pro-gated surface).
// Action-blocking, not launch-blocking — the war-room rejected
// "intercept on app start" because it punishes free users on every
// cold launch even when they're not trying to use a Pro feature.
//
// Component is INERT until wired. The simplest integration point
// is to render once at the app shell and let a hook flip `open`
// when a Pro feature is clicked. Wiring is deferred to a follow-up
// PR so this session doesn't collide with the parallel OSS work.

const UPGRADE_URL = "https://cal.com/willnigri/ato-onboarding";

type Props = {
  open: boolean;
  onClose: () => void;
};

export default function TrialExpiredModal({ open, onClose }: Props) {
  const trial = useTrialStatus();
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
        </div>

        <footer className="flex items-center justify-between gap-3 px-5 pb-5">
          <button
            type="button"
            onClick={onClose}
            className="text-xs text-cs-muted hover:text-cs-text"
          >
            Not now
          </button>
          <a
            href={UPGRADE_URL}
            target="_blank"
            rel="noreferrer noopener"
            className="inline-flex items-center gap-1.5 rounded-lg bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent/90"
          >
            Upgrade <ExternalLink size={12} aria-hidden />
          </a>
        </footer>
      </div>
    </div>
  );
}
