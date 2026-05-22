import { useTranslation } from "react-i18next";
import { Crown, X, Check, ExternalLink } from "lucide-react";
import { tierForFeature, TIER_LABEL, type Feature, type Tier } from "@/lib/tier";
import { UPGRADE_URL } from "@/lib/constants";

// v1.4.0 — Upgrade prompt modal.
//
// Shown when a Free user clicks a TierGate-locked surface. Frames what they
// unlock, names the price, and routes to the cloud auth flow. Per-feature
// copy lives in the FEATURE_COPY map below — keep these short and concrete
// (the user is one click from upgrading; brevity wins).

// Pricing is held at $0 during the beta — Pro/Team capabilities are unlocked
// via a founder-led onboarding call (Cal.com link in the CTA) rather than a
// fake checkout. Founding-user pricing will be grandfathered when paid tiers
// switch on. Anything claiming a dollar amount today is a trust leak (the
// 2026-05-16 design seat scored the old copy 2/10).
// v2.8.x — real prices after the silent-grant removal (war-room
// 87E6CADF round 3, doctrine LOCKED 2026-05-22). Grandfather $14/mo
// for first-100 alpha users is applied at Stripe-checkout time via
// coupon; it's not shown here so the public copy stays honest.
const TIER_PRICE: Record<Tier, string> = {
  free: "Free forever",
  pro: "$29 / seat / month",
  team: "$49 / seat / month",
  enterprise: "Talk to us",
};

// v2.8.x — entries for features that re-tiered to Free (variables.advanced,
// context-hooks, summarizer.tunable, groups.unlimited, groups.editor,
// role-models, evaluators ad-hoc) are REMOVED. They never gate now, so
// their upgrade copy is dead code. Doctrine 87E6CADF: "scarcity in cloud,
// not in local" — if it never gates, it never needs an upgrade prompt.
const FEATURE_COPY: Partial<Record<Feature, { title: string; bullets: string[] }>> = {
  "cloud-traces": {
    title: "Cloud trace retention + regression detection",
    bullets: [
      "Every dispatch uploads to the cloud — search, replay, compare",
      "Cross-runtime regression detection: catch the moment claude > gemini",
      "Cost-per-dispatch dashboard: which model + MCP is burning your budget?",
      "Pro: 30 days retention · Team: 90 days · Enterprise: unlimited",
    ],
  },
  "cloud-sync": {
    title: "Your agents follow you across devices",
    bullets: [
      "Edit on your laptop, run on your desktop — same agent, same config",
      "First step toward team sharing on the Team tier",
      "Works for agents, skills, and MCPs",
    ],
  },
  "provider-keys": {
    title: "Encrypted provider-key store for usage polling",
    bullets: [
      "ATO holds your API keys (encrypted) so the cloud poller can fetch",
      "real spend from Anthropic / OpenAI / Google billing endpoints",
      "Team-only — credential custody belongs at the highest trust threshold",
    ],
  },
  "team-workspaces": {
    title: "Team workspaces & shared agents",
    bullets: [
      "Share agents, skills, MCPs across your team",
      "Per-member roles + activity timeline",
      "Cloud audit aggregated across team members",
    ],
  },
  // v2.8.x — even though `evaluators` (ad-hoc) is now Free, the
  // scheduled-batch variant requires cloud cron infra. Keep this
  // copy in place for the "Run on schedule" toggle in EvaluatorsTab.
  "evaluators.scheduled": {
    title: "Scheduled batch evaluators",
    bullets: [
      "Run your eval suite hourly / daily / weekly — automated",
      "Catch regressions overnight, before users feel them in the morning",
      "Ad-hoc evaluator runs stay Free; the cron worker is the Pro piece",
    ],
  },
  "enterprise.evaluator-budgets": {
    title: "Custom evaluators with token budgets",
    bullets: [
      "Cap evaluator spend per agent / per team / per month",
      "Alert and rollover; no surprises on month-end",
    ],
  },
  "enterprise.halo": {
    title: "HALO trace optimization",
    bullets: [
      "Anthropic-style RLM optimization on your traces",
      "Surfaces harness-level failure modes, not just per-call",
      "One click to apply a suggested prompt diff",
    ],
  },
  "enterprise.sso": {
    title: "Enterprise SSO + audit",
    bullets: [
      "SAML / OIDC via Google Workspace, Okta, Microsoft Entra",
      "SOC2-aligned audit retention (unlimited)",
    ],
  },
};

interface Props {
  feature: Feature;
  open: boolean;
  onClose: () => void;
}

export default function UpgradePrompt({ feature, open, onClose }: Props) {
  const { t } = useTranslation();
  if (!open) return null;

  const requiredTier = tierForFeature(feature);
  const copy = FEATURE_COPY[feature];
  const title = copy?.title ?? t("tier.upgrade", "Upgrade");
  const bullets = copy?.bullets ?? [];

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-[55] flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-md rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-start justify-between p-5 border-b border-cs-border">
          <div className="flex items-start gap-3 min-w-0">
            <div className="w-9 h-9 rounded-lg bg-cs-accent/10 flex items-center justify-center shrink-0">
              <Crown size={16} className="text-cs-accent" />
            </div>
            <div className="min-w-0">
              <h2 className="text-sm font-semibold text-cs-text">{title}</h2>
              <p className="text-[11px] text-cs-muted mt-0.5">
                {t("tier.requires", "Requires {{tier}} · {{price}}", {
                  tier: TIER_LABEL[requiredTier],
                  price: TIER_PRICE[requiredTier],
                })}
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text shrink-0"
          >
            <X size={18} />
          </button>
        </header>

        <div className="p-5">
          {bullets.length > 0 && (
            <ul className="space-y-2">
              {bullets.map((b, i) => (
                <li key={i} className="flex items-start gap-2 text-sm text-cs-text">
                  <Check size={14} className="text-cs-accent shrink-0 mt-0.5" />
                  <span>{b}</span>
                </li>
              ))}
            </ul>
          )}
        </div>

        <footer className="flex items-center justify-between gap-3 px-5 pb-5">
          <button
            type="button"
            onClick={onClose}
            className="text-xs text-cs-muted hover:text-cs-text"
          >
            {t("tier.notNow", "Not now")}
          </button>
          <a
            href={UPGRADE_URL}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover"
          >
            {t("tier.bookOnboarding", "Book onboarding · free")}
            <ExternalLink size={12} />
          </a>
        </footer>
      </div>
    </div>
  );
}
