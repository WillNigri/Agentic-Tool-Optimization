import { useEffect, useState } from "react";
import { useAuthStore, type Tier } from "@/hooks/useAuth";
import {
  deriveTrialStatus,
  hasEverPaid,
  readTrialStartedAt,
  startTrialIfUnset,
  type TrialStatus,
} from "@/lib/trial";

// v1.4.0 — Tier helper.
//
// Single source of truth for which subscription tier each feature requires.
// `<TierGate feature="...">` and `useFeatureFlag()` both read from this map.
//
// UI rule (from the v1.4 plan): Pro features are VISIBLE to Free users with a
// crown lock badge + upgrade tooltip — discovery sells. Don't hide.
//
// Phase 1 PR-A (2026-05-21) — alpha free-Pro grant becomes a 14-day trial.
// `useTier` still resolves to "pro" for Tauri/cloud users with a free cache,
// but only inside the trial window. Trial state lives in `lib/trial.ts`.

export type { Tier };

export const TIER_ORDER: Record<Tier, number> = {
  free: 0,
  pro: 1,
  team: 2,
  enterprise: 3,
};

export const TIER_LABEL: Record<Tier, string> = {
  free: "Free",
  pro: "Pro",
  team: "Team",
  enterprise: "Enterprise",
};

/** Every gated capability. Add a new entry here when introducing a paid
 *  feature, then wrap the UI with <TierGate feature="..."> or check
 *  useFeatureFlag() before allowing the action. */
export type Feature =
  // F1 — Variables (advanced resolvers; basic ones stay free)
  | "variables.advanced"
  // F2 — Pre-call context hooks
  | "context-hooks"
  // F3 — Tunable summarizer policy (basic threshold stays free)
  | "summarizer.tunable"
  // F4 — Multi-agent groups
  | "groups.unlimited"        // free is capped at 3 children
  | "groups.editor"           // editor vs view-only
  // F5 — Per-task model selection
  | "role-models"
  // F6 — Cloud trace retention (local trace view stays free)
  | "cloud-traces"
  // F7 — Evaluators
  | "evaluators"
  // Cloud sync of agents
  | "cloud-sync"
  // v2.6 — Encrypted provider-key store for the cron usage-poller
  | "provider-keys"
  // Team workspaces
  | "team-workspaces"
  // Enterprise-only
  | "enterprise.evaluator-budgets"
  | "enterprise.halo"
  | "enterprise.sso"
  | "enterprise.audit";

export const FEATURE_MIN_TIER: Record<Feature, Tier> = {
  "variables.advanced": "pro",
  "context-hooks": "pro",
  "summarizer.tunable": "pro",
  "groups.unlimited": "pro",
  "groups.editor": "pro",
  "role-models": "pro",
  "cloud-traces": "pro",
  "evaluators": "pro",
  "cloud-sync": "pro",
  "provider-keys": "pro",
  "team-workspaces": "team",
  "enterprise.evaluator-budgets": "enterprise",
  "enterprise.halo": "enterprise",
  "enterprise.sso": "enterprise",
  "enterprise.audit": "enterprise",
};

function isTauriContext(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Compute the effective tier from cached tier + trial status.
 *  Pure helper for tests; the hook below wraps it with React state. */
export function resolveEffectiveTier(
  cachedTier: Tier,
  isCloudUser: boolean,
  trial: TrialStatus,
  isTauri: boolean,
  everPaid: boolean,
): Tier {
  // Paid tiers (Pro / Team / Enterprise) always win — never downgrade.
  if (cachedTier !== "free") return cachedTier;
  // Trial only applies in the Tauri/cloud context that originally
  // qualified for the alpha grant. Pure web visitors stay Free.
  if (!isTauri && !isCloudUser) return "free";
  // Once a user has held paid Pro, they don't get a fresh trial
  // after sign-out — that loophole would let anyone re-arm 14 more
  // days by signing out and back in. The `everPaid` latch is wired
  // from the auth flow (markEverPaid in trial.ts) in a follow-up PR;
  // it stays false today, so this branch is currently inert by design.
  if (everPaid) return "free";
  return trial.state === "active" ? "pro" : "free";
}

/** Hook: reads the current user's effective tier.
 *
 *  Phase 1 PR-A (2026-05-21) — alpha free-Pro grant becomes a
 *  14-day trial. Returns "pro" only when:
 *    - the cached tier is already paid (Team / Enterprise / Pro), OR
 *    - the user is in Tauri or cloud-authed AND the trial window
 *      is active.
 *
 *  Starting the trial is a side effect — first call from a Tauri
 *  or cloud-auth context with a free cache sets `trialStartedAt`
 *  if missing. We do this here (rather than at "first Pro feature
 *  view") because every Pro-gated surface calls useTier; the first
 *  view through a TierGate is the trigger by construction. */
export function useTier(): Tier {
  const cachedTier = useAuthStore((s) => s.tier);
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const isTauri = isTauriContext();

  const [trial, setTrial] = useState<TrialStatus>(() =>
    deriveTrialStatus(readTrialStartedAt()),
  );

  useEffect(() => {
    // Only start the clock when the user actually qualifies for
    // the trial — not for plain web visitors or paid users.
    if (cachedTier !== "free") return;
    if (!isTauri && !isCloudUser) return;
    if (hasEverPaid()) return;
    const startedAt = startTrialIfUnset();
    if (startedAt !== trial.startedAt) {
      setTrial(deriveTrialStatus(startedAt));
    }
  }, [cachedTier, isCloudUser, isTauri, trial.startedAt]);

  return resolveEffectiveTier(
    cachedTier,
    isCloudUser,
    trial,
    isTauri,
    hasEverPaid(),
  );
}

/** Hook: trial countdown for UI surfaces (banner, modal, tooltips).
 *  Independent of `useTier` so a paid user reading the banner state
 *  doesn't accidentally start a trial. */
export function useTrialStatus(): TrialStatus {
  const [trial, setTrial] = useState<TrialStatus>(() =>
    deriveTrialStatus(readTrialStartedAt()),
  );
  useEffect(() => {
    // Re-derive on mount in case another hook started the trial
    // between the first render and the effect. Cheap; pure.
    setTrial(deriveTrialStatus(readTrialStartedAt()));
  }, []);
  return trial;
}

/** Hook: returns true when the current tier is high enough for `feature`. */
export function useFeatureFlag(feature: Feature): boolean {
  const tier = useTier();
  return TIER_ORDER[tier] >= TIER_ORDER[FEATURE_MIN_TIER[feature]];
}

/** Lower-level helper for non-React code paths (e.g. inside lib/ helpers). */
export function tierMeetsRequirement(currentTier: Tier, required: Tier): boolean {
  return TIER_ORDER[currentTier] >= TIER_ORDER[required];
}

/** What does a feature require? Use to render upgrade prompts. */
export function tierForFeature(feature: Feature): Tier {
  return FEATURE_MIN_TIER[feature];
}
