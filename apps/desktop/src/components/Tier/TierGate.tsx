import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Crown, Lock } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  useFeatureFlag,
  useTrialStatus,
  tierForFeature,
  TIER_LABEL,
  type Feature,
} from "@/lib/tier";
import { featureRoiCopy } from "@/lib/featureRoiCopy";
import UpgradePrompt from "./UpgradePrompt";
import TrialExpiredModal from "@/components/Trial/TrialExpiredModal";

// v1.4.0 — TierGate.
//
// Wraps children that require a specific Pro / Team / Enterprise feature.
// Free users see the children rendered as a "soft-locked" placeholder with
// a crown badge + click → opens UpgradePrompt. We don't hide the surface —
// discovery sells.
//
// Usage:
//   <TierGate feature="context-hooks">
//     <ContextHooksTab agentId={id} />
//   </TierGate>
//
// `mode` controls the lock visual:
//   - "block" (default): renders an inline locked panel instead of children
//   - "overlay": renders children but with a translucent overlay + crown
//   - "field": for inline form fields — renders a small lock icon + disables
//     the children. Caller is responsible for actually disabling controls.

interface Props {
  feature: Feature;
  children: ReactNode;
  /** What gets shown when locked. */
  mode?: "block" | "overlay" | "field";
  /** Optional one-line custom hint shown in the locked state. */
  hint?: string;
}

export default function TierGate({ feature, children, mode = "block", hint }: Props) {
  const allowed = useFeatureFlag(feature);
  const trial = useTrialStatus();
  const { t } = useTranslation();
  const [promptOpen, setPromptOpen] = useState(false);
  const requiredTier = tierForFeature(feature);

  if (allowed) return <>{children}</>;

  const tierLabel = TIER_LABEL[requiredTier];
  // PR-F (2026-05-21) — Crown badge tooltips lead with the ROI / capability
  // story per feature, not the generic "Upgrade to Pro" copy. The caller's
  // explicit `hint` still wins (some surfaces need contextual copy);
  // featureRoiCopy is the next-best default; generic i18n is the fallback
  // for any future Feature key added without an ROI line. War-room
  // de8ffb6d-8b39-4b5c-a2e9-6665e6e7e9f3, R1 Q4 3/3 LOCK.
  const roiCopy = featureRoiCopy(feature);
  const tooltip =
    hint ??
    roiCopy ??
    t("tier.lockedTooltip", "Upgrade to {{tier}} to unlock", { tier: tierLabel });

  // Phase 1 PR-B — when the 14-day Pro trial has expired, the click on
  // a locked surface triggers the trial-expired modal instead of the
  // per-feature UpgradePrompt. The trial modal frames the loss ("your
  // Pro trial has ended") rather than the unlock ("unlock context
  // hooks"), which converts better at the expiry moment. Same promptOpen
  // state — no new store, follows the existing per-TierGate modal pattern.
  const prompt =
    trial.state === "expired" ? (
      <TrialExpiredModal open={promptOpen} onClose={() => setPromptOpen(false)} />
    ) : (
      <UpgradePrompt feature={feature} open={promptOpen} onClose={() => setPromptOpen(false)} />
    );

  if (mode === "overlay") {
    return (
      <>
        <div
          className="relative"
          role="button"
          tabIndex={0}
          onClick={() => setPromptOpen(true)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") setPromptOpen(true);
          }}
        >
          <div className="pointer-events-none opacity-50 select-none">{children}</div>
          <div
            className="absolute inset-0 flex items-center justify-center bg-cs-bg/40 backdrop-blur-[1px] cursor-pointer hover:bg-cs-bg/30 transition"
            title={tooltip}
          >
            <span className="inline-flex items-center gap-1.5 rounded-full border border-cs-accent/40 bg-cs-bg-raised px-3 py-1 text-xs font-medium text-cs-accent">
              <Crown size={12} />
              {tierLabel}
            </span>
          </div>
        </div>
        {prompt}
      </>
    );
  }

  if (mode === "field") {
    return (
      <>
        <span
          className="inline-flex items-center gap-1 cursor-pointer"
          onClick={() => setPromptOpen(true)}
          title={tooltip}
        >
          <Lock size={11} className="text-cs-muted" />
          <span className="text-[10px] uppercase tracking-wide text-cs-muted">
            <Crown size={10} className="inline mr-0.5 -mt-0.5 text-cs-accent" />
            {tierLabel}
          </span>
        </span>
        {prompt}
      </>
    );
  }

  // mode === "block"
  return (
    <>
      <button
        type="button"
        onClick={() => setPromptOpen(true)}
        className={cn(
          "w-full rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6",
          "flex items-center gap-3 text-left hover:border-cs-accent/40 hover:bg-cs-accent/5 transition"
        )}
      >
        <div className="w-10 h-10 rounded-full bg-cs-accent/10 flex items-center justify-center shrink-0">
          <Crown size={16} className="text-cs-accent" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-cs-text">
            {t("tier.lockedTitle", "{{tier}} feature", { tier: tierLabel })}
          </p>
          <p className="mt-1 text-xs text-cs-muted">{tooltip}</p>
        </div>
        <span className="text-xs text-cs-accent font-medium shrink-0">
          {t("tier.upgrade", "Upgrade")} →
        </span>
      </button>
      {prompt}
    </>
  );
}
