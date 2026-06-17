/**
 * MigrationBanner — chunk 4 of Phase A (war-room 87E6CADF round 3,
 * DevEx AMEND: "lead with the GAIN, not the loss").
 *
 * Shown once to existing users post-v2.8.x re-tier (commits 76d856a +
 * af6daae). Surfaces the wins (6 features now Free, 3-child cap killed)
 * before users see the new $29 / $49 prices on UpgradePrompt and assume
 * we stole features.
 *
 * Dismissal is one-shot via localStorage. The banner renders defensively
 * — if localStorage is unavailable (locked-down webview), we render
 * nothing rather than throw on mount.
 *
 * Original draft contributed by minimax via parallel-engineering workflow
 * (Will authorized 2026-05-22). Integrated + polished by claude:
 *   - Real GitHub URL for "See what's new"
 *   - Theme color tokens (`bg-cs-*`) where they exist
 *   - aria-modal + tabIndex on headline for screen-reader entry
 */
import { useState, useEffect } from "react";
import { ExternalLink, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
// useTrialStatus lives in `@/lib/tier`, not `@/lib/trial` (trial.ts holds
// only the pure helpers; the hook is co-located with useTier in tier.ts).
import { useTrialStatus } from "@/lib/tier";

// Bumped 2026-06-16 from v2.8.x → v2.18 — Will flagged the banner
// was still showing the stale v2.8.x re-tier copy after the v2.18
// cluster shipped. New storage key forces existing users to see the
// updated banner once (their previous dismissal of the v2.8.x
// version doesn't suppress the v2.18 version).
const STORAGE_KEY = "ato.v2.18.x.banner.dismissed";

// Current release notes — points at the live CHANGELOG so the
// banner copy stays in sync with what shipped.
const SEE_WHATS_NEW_URL =
  "https://github.com/WillNigri/Agentic-Tool-Optimization/blob/main/CHANGELOG.md";

function isLocalStorageAvailable(): boolean {
  try {
    if (typeof window === "undefined" || !window.localStorage) return false;
    window.localStorage.setItem("__ato_test__", "1");
    window.localStorage.removeItem("__ato_test__");
    return true;
  } catch {
    return false;
  }
}

function wasBannerDismissed(): boolean {
  if (!isLocalStorageAvailable()) return true; // fail-closed: don't nag if storage is locked
  return window.localStorage.getItem(STORAGE_KEY) === "true";
}

function dismissBanner(): void {
  if (!isLocalStorageAvailable()) return;
  window.localStorage.setItem(STORAGE_KEY, "true");
}

export default function MigrationBanner() {
  const { t } = useTranslation();
  const { state, startedAt } = useTrialStatus();
  const [dismissed, setDismissed] = useState(true); // start dismissed so first paint is empty

  // Read localStorage post-mount only — SSR / hydration safety.
  useEffect(() => {
    setDismissed(wasBannerDismissed());
  }, []);

  if (dismissed) return null;

  const trialDay = (() => {
    if (state !== "active" || !startedAt) return 0;
    const start = new Date(startedAt).getTime();
    const dayMs = 24 * 60 * 60 * 1000;
    const days = Math.floor((Date.now() - start) / dayMs) + 1;
    return Math.min(Math.max(days, 1), 14);
  })();

  const handleDismiss = () => {
    dismissBanner();
    setDismissed(true);
  };

  return (
    <div
      className="relative border-b border-cs-accent/20 bg-gradient-to-r from-cs-bg-raised to-cs-bg"
      role="region"
      aria-label={t("migration.ariaLabel", "ATO update notification")}
    >
      <div className="mx-auto max-w-5xl px-4 py-4 sm:px-6 lg:px-8">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0 flex-1">
            <div className="mb-1 flex items-center gap-2">
              <Sparkles size={14} className="shrink-0 text-cs-accent" />
              <span className="text-[10px] font-medium uppercase tracking-wider text-cs-accent">
                {t("migration.update", "Update — v2.18")}
              </span>
            </div>

            <h2
              tabIndex={-1}
              className="mb-2 text-base font-bold text-cs-text focus:outline-none sm:text-lg"
            >
              {t("migration.headline", "v2.18 is here — your war room reaches the browser")}
            </h2>

            <p className="mb-3 text-sm text-cs-muted">
              {t(
                "migration.subheadline",
                "The cluster you've been seeing in the changelog: web Team Workspaces, browser ⇄ desktop tether, multi-LLM methodology runs with real cost ledgers. All Free.",
              )}
            </p>

            <ul className="mb-4 space-y-1.5">
              {[
                t(
                  "migration.bullet.webWorkspaces",
                  "v2.16 + v2.17 — read-only Team Workspaces on the web, then browser ⇄ desktop AEAD-sealed tether",
                ),
                t(
                  "migration.bullet.activeWorkstation",
                  "v2.18 Wave 1 — browser-driven dispatch over the tether (claude only for Wave 1; more runtimes incoming)",
                ),
                t(
                  "migration.bullet.methodology",
                  "v2.10 — Methodology runner: reusable test recipes, Welch t + 95% CI, dual cost ledger",
                ),
                t(
                  "migration.bullet.missions",
                  "v2.16 — Missions (proactive coordinator): coordinator tick, worktrees, merge strategies, decision briefs, board UI",
                ),
              ].map((bullet, i) => (
                <li
                  key={i}
                  className="flex items-start gap-2 text-sm text-cs-text"
                >
                  <span className="mt-0.5 text-cs-accent" aria-hidden="true">
                    ✓
                  </span>
                  <span>{bullet}</span>
                </li>
              ))}
            </ul>

            {state === "active" && trialDay > 0 && (
              <p className="mb-4 text-sm text-cs-muted">
                {t("migration.trialStatus", {
                  defaultValue: "You're on day {{day}} of 14 of your Pro trial — try cloud-traces + cloud-sync before it ends.",
                  day: trialDay,
                })}
              </p>
            )}

            <div className="flex flex-wrap items-center gap-3">
              <a
                href={SEE_WHATS_NEW_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-semibold text-cs-bg transition-colors hover:bg-cs-accent-hover focus:outline-none focus:ring-2 focus:ring-cs-accent focus:ring-offset-2 focus:ring-offset-cs-bg"
              >
                {t("migration.seeWhatsNew", "See what's new")}
                <ExternalLink size={12} aria-hidden="true" />
              </a>
              <button
                type="button"
                onClick={handleDismiss}
                className="inline-flex items-center gap-2 rounded-lg px-3 py-2 text-sm text-cs-muted transition-colors hover:bg-cs-bg-raised/40 hover:text-cs-text focus:outline-none focus:ring-2 focus:ring-cs-border"
                aria-label={t("migration.dismiss", "Dismiss this notification")}
              >
                {t("migration.dismiss", "Dismiss")}
              </button>
            </div>

            <p className="mt-4 text-xs italic text-cs-muted">
              {t(
                "migration.proContinues",
                "Pro continues to cover cloud-traces and cross-device sync — the parts that actually cost us to host.",
              )}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
