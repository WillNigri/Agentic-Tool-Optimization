// Centralized app-wide URLs.
//
// Phase 1 PR-B (2026-05-21): created to deduplicate the upgrade-flow URL
// previously hardcoded in UpgradePrompt.tsx, TrialBanner.tsx, and
// TrialExpiredModal.tsx. Three call sites + zero source of truth = a
// future move of cal.com/willnigri would silently leave one banner
// pointing at the old link.
//
// Only UPGRADE_URL lives here today (YAGNI). When the next constant has
// a real caller, add it alongside.

/** Founder-led onboarding call. The only upgrade path during the
 *  Phase-1 beta — no Stripe checkout yet. Pricing is "free during
 *  beta" with founding-user grandfathering when paid tiers switch on. */
export const UPGRADE_URL = "https://cal.com/willnigri/ato-onboarding";
