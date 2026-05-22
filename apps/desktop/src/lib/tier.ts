import { useEffect } from "react";
import { useAuthStore, type Tier } from "@/hooks/useAuth";
import { recordFeatureUse } from "@/lib/conversionTelemetry";

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

/** Hook: reads the current user's effective tier.
 *
 *  v1.5.x early-access promo — anyone with a cloud account gets Pro for
 *  free. Drives signups + dogfood while we collect feedback. Local-only
 *  users (no account) stay on Free, which is the upgrade hook on the UI.
 *  Team / Enterprise carriers keep their cached tier (no downgrade).
 *
 *  v2.0.0 — Tauri desktop users also default to Pro (no cloud login
 *  needed). Beatriz feedback: she couldn't log in locally and was
 *  blocked from testing Pro features in dev mode. The desktop install
 *  is the trusted path during the alpha; we'll re-tier when we ship
 *  paid Pro post-alpha.
 *
 *  When we re-introduce paid Pro, remove both the cloud and Tauri
 *  branches. */
export function useTier(): Tier {
  const cachedTier = useAuthStore((s) => s.tier);
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  if ((isTauri || isCloudUser) && cachedTier === "free") return "pro";
  return cachedTier;
}

/** Hook: returns true when the current tier is high enough for `feature`.
 *
 *  Strategy PR-B (2026-05-21) — every mount of a gated UI bumps a
 *  conversion counter via `recordFeatureUse`. The bump fires from a
 *  `useEffect`, NOT during render — code-review war-room (claude/minimax
 *  seats, 2026-05-22) caught that a render-time bump turns "user opened
 *  panel" into "React re-rendered this many times" and corrupts the WTP
 *  signal. Effect re-fires only on (feature, tier) change so a
 *  mid-session tier flip starts a new counter; otherwise one mount =
 *  one bump (plus the dev StrictMode double-mount, which is contained
 *  to dev). The counter flushes to SQLite every 60s; see
 *  `lib/conversionTelemetry.ts` for the flush model. */
export function useFeatureFlag(feature: Feature): boolean {
  const tier = useTier();
  useEffect(() => {
    recordFeatureUse(feature, tier);
  }, [feature, tier]);
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
