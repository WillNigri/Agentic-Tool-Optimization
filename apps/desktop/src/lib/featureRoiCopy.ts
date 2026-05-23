import type { Feature } from "@/lib/tier";

// PR-F (2026-05-21) — Per-feature ROI tooltip copy for crown badges.
//
// TierGate's default locked tooltip used to read "Upgrade to Pro to unlock"
// for every gated surface. A 2026-05-16 design seat scored that generic copy
// 2/10: it tells the user *what tier* they need but not *why* they'd want it.
// This map gives every Feature a one-line value proposition that the crown
// badge / lock chip surfaces on hover, before the user clicks through to
// UpgradePrompt's longer pitch.
//
// Tone rules (locked in war-room de8ffb6d-8b39-4b5c-a2e9-6665e6e7e9f3 Q4):
//   - When a concrete $/mo or % story exists, lead with the number.
//   - Otherwise, lead with the capability the user can't get otherwise
//     (compliance, scale, multi-device). Never fabricate a dollar amount
//     just to make the map look uniform.
//   - One line, ≤ 90 chars after wrapping. Plain English, no jargon.
//   - These are tooltips, not pitches — UpgradePrompt holds the long form.

const COPY: Record<Feature, string> = {
  // F1 — Variables
  "variables.advanced":
    "Dynamic resolvers pull from MCP / DB / files — no manual edits per turn",

  // F2 — Pre-call context hooks
  "context-hooks":
    "Pre-call hooks inject fresh data each turn — CRM-as-context, no server needed",

  // F3 — Summarizer policy
  "summarizer.tunable":
    "Tunable summarizer + cheaper summary model — cuts ~30% of tokens on long sessions",

  // F4 — Groups
  "groups.unlimited":
    "Free caps groups at 3 children; Pro is unlimited — specialization beats one mega-agent",
  "groups.editor":
    "Visual graph editor: drag children, edit router, preview routes without re-running",

  // F5 — Per-task model selection
  "role-models":
    "Sonnet to plan + Haiku to execute drops cost ~40% vs single-model dispatches",

  // F6 — Cloud trace retention
  "cloud-traces":
    "30-day cross-device retention — surfaces ~$8/mo of cost wins the local view misses",

  // F7 — Evaluators
  "evaluators":
    "LLM-as-judge scoring catches regressions before they reach production",
  "evaluators.scheduled":
    "Run your eval suite hourly / daily / weekly — automated regression detection",

  // Cloud sync
  "cloud-sync":
    "Agents + skills sync across devices — never re-configure on a new laptop",

  // v2.6 — Encrypted provider-key store
  "provider-keys":
    "Encrypted key store powers the cron usage-poller — ~$0.02/day to run on your keys",

  // Team
  "team-workspaces":
    "Shared agents + skills across teammates, with per-seat trace attribution",

  // Enterprise — no $/mo story; lead with capability / compliance.
  "enterprise.evaluator-budgets":
    "Per-team eval spend caps prevent runaway LLM-as-judge costs",
  "enterprise.halo":
    "Org-wide safety guardrails layered across every dispatch",
  "enterprise.sso":
    "Required for SSO / SAML org rollouts",
  "enterprise.audit":
    "Required for SOC2 audit trail retention",
};

/** Returns the one-line ROI tooltip for a Feature.
 *  Falls back to `null` when no copy is registered — the caller should then
 *  use the generic "Upgrade to {tier}" i18n string. In practice every Feature
 *  in tier.ts has copy here; the null branch exists so a TypeScript-broken
 *  feature key at runtime degrades gracefully instead of crashing. */
export function featureRoiCopy(feature: Feature): string | null {
  return COPY[feature] ?? null;
}
