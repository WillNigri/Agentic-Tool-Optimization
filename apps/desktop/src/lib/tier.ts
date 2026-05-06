import { useAuthStore, type Tier } from "@/hooks/useAuth";

// v1.4.0 — Tier helper.
//
// Single source of truth for which subscription tier each feature requires.
// `<TierGate feature="...">` and `useFeatureFlag()` both read from this map.
//
// UI rule (from the v1.4 plan): Pro features are VISIBLE to Free users with a
// crown lock badge + upgrade tooltip — discovery sells. Don't hide.

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
  "team-workspaces": "team",
  "enterprise.evaluator-budgets": "enterprise",
  "enterprise.halo": "enterprise",
  "enterprise.sso": "enterprise",
  "enterprise.audit": "enterprise",
};

/** Hook: reads the current user's tier from the auth store. */
export function useTier(): Tier {
  return useAuthStore((s) => s.tier);
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
