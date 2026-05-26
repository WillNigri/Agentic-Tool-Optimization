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

/** Founder-led onboarding call. No-JWT fallback only — when a user is
 *  signed in, `lib/billing.ts#startCheckout` opens a real Stripe Checkout
 *  session instead. Local-only users (no cloud account) still need
 *  somewhere to land, and the Calendly path stays as the conversion
 *  surface for them. Also used as the safety net on 402 PRO_REQUIRED
 *  (account isn't eligible for self-serve). */
export const UPGRADE_URL = "https://cal.com/willnigri/ato-onboarding";
