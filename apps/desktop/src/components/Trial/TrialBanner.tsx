import { useEffect, useState } from "react";
import { Clock, X, ExternalLink } from "lucide-react";

import { useTrialStatus } from "@/lib/tier";
import { TRIAL_BANNER_DISMISSED_KEY } from "@/lib/trial";
import { UPGRADE_URL } from "@/lib/constants";

// Phase 1 PR-A — persistent trial countdown banner.
//
// Renders only from day 7 of the trial onward (the war-room's
// "first user-visible reminder" decision). Dismissal is per-
// session, not per-day, so users who close it once still see it
// next launch — the cost of a missed conversion outweighs the
// annoyance of a single re-show.
//
// Wired in PR-B at the top of Dashboard's content column (above
// <main>) so it persists across section scrolls and only shows
// inside the authenticated app chrome.

export default function TrialBanner() {
  const trial = useTrialStatus();
  const [dismissed, setDismissed] = useState(false);

  // Re-read the sessionStorage marker on mount so a previous tab's
  // dismissal isn't ignored if the user opens a second window.
  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      if (window.sessionStorage.getItem(TRIAL_BANNER_DISMISSED_KEY) === "1") {
        setDismissed(true);
      }
    } catch {
      // sessionStorage unavailable — banner just stays visible. Fine.
    }
  }, []);

  if (!trial.showBanner || dismissed) return null;

  const dismiss = () => {
    setDismissed(true);
    try {
      window.sessionStorage.setItem(TRIAL_BANNER_DISMISSED_KEY, "1");
    } catch {
      // Best-effort. State is already updated; banner will reappear
      // next session if the write failed.
    }
  };

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex items-center gap-3 border-b border-cs-border bg-cs-accent/5 px-4 py-2 text-xs"
    >
      <Clock size={14} className="text-cs-accent shrink-0" aria-hidden />
      <span className="text-cs-text">
        Trial Pro: <strong>{trial.daysRemaining} day{trial.daysRemaining === 1 ? "" : "s"}</strong> remaining.
      </span>
      <a
        href={UPGRADE_URL}
        target="_blank"
        rel="noreferrer noopener"
        className="ml-auto inline-flex items-center gap-1 text-cs-accent hover:underline"
      >
        Upgrade <ExternalLink size={11} aria-hidden />
      </a>
      <button
        type="button"
        aria-label="Dismiss trial banner"
        onClick={dismiss}
        className="text-cs-muted hover:text-cs-text shrink-0"
      >
        <X size={14} />
      </button>
    </div>
  );
}
