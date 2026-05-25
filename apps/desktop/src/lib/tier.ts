import { useEffect, useState } from "react";
import { useAuthStore, type Tier } from "@/hooks/useAuth";
import {
  deriveTrialStatus,
  hasEverPaid,
  readTrialStartedAt,
  startTrialIfUnset,
  type TrialStatus,
} from "@/lib/trial";
// v2.8.x chunk 5 — telemetry-as-value-exchange (Will's "we have no
// telemetry" correction). conv-telemetry branch added the local
// recording side; cloud-forward lives in conversionTelemetry.ts.
import { recordFeatureUse } from "@/lib/conversionTelemetry";

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
  | "evaluators"                  // ad-hoc evaluator runs (Free; local-only)
  | "evaluators.scheduled"        // cron-driven batch evals (Pro; cloud cron)
  // v2.10 PR-8 — Methodology runner. Local execution (CLI `ato evaluations
  // methodology run`, local cron `schedule create`) stays FREE per the
  // "scarcity in cloud, not in BYOK/local" doctrine. Pro gate covers
  // cloud sync of methodology runs (cross-device history, team sharing,
  // hosted scheduled runs with email alerts). The OSS Insights panel
  // never calls cloud endpoints — it reads local SQLite only.
  | "methodology.cloud-sync"
  // v2.11 PR-12.05 — open-core re-tier. The PRINCIPLE Will locked
  // 2026-05-25: customers can run primitives free; we charge for the
  // codified automation we package on top. Schedule was shipped free
  // in v2.10 PR-7; under the open-core principle it's automation we
  // provide (customer could write their own launchd plist by hand)
  // and it's gated Pro for new creates from PR-12.05 onward.
  // Existing schedules are grandfathered: list / delete / trigger /
  // unarchive remain free so customers can manage what they already
  // set up. See docs/v2.11-learning-loop.md and docs/tiers.md.
  | "methodology.schedule"
  // v2.11 PR-12 (in progress) — methodology diagnose pipeline. The
  // codified learning-loop button (reads failing methodology cells,
  // proposes a structured agent-definition change, A/B tests it,
  // ships if Pareto-better). Pro from day one.
  | "methodology.diagnose"
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
  // ─── FREE (local power, unlimited) ─────────────────────────────
  // v2.8.x re-tier (war-room 87E6CADF round 3, doctrine locked
  // 2026-05-22): "scarcity in cloud, not in BYOK / local." Anything
  // that runs entirely on the user's Mac with the user's API keys
  // is FREE — we add zero incremental infra cost, so charging for
  // it is the artificial scarcity the doctrine forbids.
  "variables.advanced": "free",   // local resolvers (file/db/mcp/computed)
  "context-hooks": "free",        // local pre-call hooks
  "summarizer.tunable": "free",   // local model picker
  "groups.unlimited": "free",     // ⚠ 3-child cap killed — see groupsCap.ts
  "groups.editor": "free",        // local group editor UI
  "role-models": "free",          // local per-task model selection
  "evaluators": "free",           // AD-HOC ONLY — single-shot, local
  "evaluators.scheduled": "pro",  // CRON-DRIVEN — requires cloud cron worker
  "methodology.cloud-sync": "pro", // CLOUD ONLY — local runs/UI stay free
  "methodology.schedule": "pro",   // AUTOMATION — re-tiered from free (v2.11 PR-12.05)
  "methodology.diagnose": "pro",   // AUTOMATION — codified learning loop

  // ─── PRO $29/seat/mo (cloud infra, real cost-of-goods) ─────────
  // These features REQUIRE ato-cloud infrastructure (storage,
  // hosted compute, cron workers). Price = cloud cost-of-goods +
  // margin. NEVER gate a feature behind Pro just because it's
  // a feature — only because it costs us money to host.
  "cloud-traces": "pro",          // trace upload + cloud aggregation
  "cloud-sync": "pro",            // cross-device sync (agents + skills)

  // ─── TEAM $49/seat/mo (multi-user + credential custody) ───────
  // war-room 87E6CADF round 3 — security-specialist AMEND: any
  // cloud feature where ATO HOLDS a user credential belongs at
  // the highest trust threshold (Team), not Pro.
  "provider-keys": "team",        // ATO holds encrypted API keys for cron poller
  "team-workspaces": "team",      // multi-user workspace primitive

  // ─── ENTERPRISE ─────────────────────────────────────────────
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
 *  HISTORY:
 *  - v1.5.x: cloud-account users silently auto-promoted to Pro (alpha promo)
 *  - v2.0.0: Tauri desktop users silently auto-promoted to Pro
 *    (Beatriz couldn't login locally; alpha trust)
 *  - PR-A (2026-05-21, commit 4638d46): silent grants replaced by a 14-day
 *    Pro trial. resolveEffectiveTier composes: paid tier wins → trial
 *    window grants "pro" → otherwise "free".
 *  - v2.8.x re-tier (2026-05-22, war-room 87E6CADF round 3): doctrine
 *    LOCKED "scarcity in cloud, not in BYOK / local." Most features that
 *    were Pro are now Free (vars / hooks / summarizer / groups /
 *    role-models / ad-hoc evaluators). What remains Pro is cloud-traces
 *    + cloud-sync — the things that actually cost us infra to host.
 *    provider-keys elevated to Team because ATO holds user credentials.
 *
 *  Starting the trial is a side effect — first call from a Tauri or
 *  cloud-auth context with a free cache sets `trialStartedAt` if
 *  missing. We do this here (rather than at "first Pro feature view")
 *  because every Pro-gated surface calls useTier; the first view through
 *  a TierGate is the trigger by construction.
 *
 *  War-room 1C5C5135 (chunk-1 code review) VETO precondition: this hook
 *  MUST compose with the trial chain — if useTier ever returns the bare
 *  cached tier, all silent-grant users hard-cut to Free without their
 *  trial. Keep `resolveEffectiveTier` in the return path. */
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
