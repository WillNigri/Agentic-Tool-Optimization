// Phase 1 PR-A — Pro trial mechanic.
//
// The alpha free-for-everyone grant at tier.ts:91-97 produced zero
// willingness-to-pay signal. Flip it to a 14-day trial: users on
// Tauri/cloud still get Pro features immediately, but the clock
// runs from first Pro-feature interaction. After 14 days the
// effective tier drops back to "free" and the conversion-block
// modal fires the next time they touch a Pro feature.
//
// Architecture (war-room 1CBFA7F2-770E-42E5-92A6-6E964F2B6E39):
//   - Trial state is DERIVED from `trialStartedAt` + 14 days. We
//     do not persist `state` or `expiresAt` separately — derived
//     fields drift the moment the clock or the duration constant
//     moves.
//   - Storage uses `localStorage` to match the existing client-
//     persistence pattern (`WelcomeTour`, `SetupWizard`). Clearing
//     localStorage resets the trial; this is an acceptable abuse
//     surface for v1 — the cloud-auth-required trial start is
//     scoped to a follow-up PR.
//   - Pure functions only. React hooks live in `tier.ts` and the
//     component layer so this module stays unit-testable without
//     a renderer.

/** Trial window in days. Exported so tests can pin the constant
 *  rather than re-deriving from epoch math. */
export const TRIAL_DURATION_DAYS = 14;

/** localStorage key — namespaced with `ato.` like
 *  `ato.welcome-tour.shown` and `ato.subtab.insights`. */
export const TRIAL_STARTED_AT_KEY = "ato.trial.startedAt";
/** Permanent flag: once a user has ever had paid Pro, they don't
 *  get a fresh trial after sign-out. */
export const TRIAL_EVER_PAID_KEY = "ato.trial.everPaid";
/** Per-session banner-dismissal marker (sessionStorage, not local). */
export const TRIAL_BANNER_DISMISSED_KEY = "ato.trial.bannerDismissed";

export type TrialState = "never-started" | "active" | "expired";

export type TrialStatus = {
  state: TrialState;
  /** ISO timestamp of the first Pro-feature interaction. Undefined
   *  when the trial hasn't started yet. */
  startedAt?: string;
  /** Whole days remaining. 0 when expired or never-started. */
  daysRemaining: number;
  /** True iff the persistent banner should render (day 7 onward). */
  showBanner: boolean;
};

/** Storage abstraction — defaults to window.localStorage but accepts
 *  an injection for tests (no jsdom global mutation needed). */
export type TrialStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function defaultStorage(): TrialStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** Compute trial status from a stored start timestamp + a clock.
 *  Pure: same inputs always produce the same output. */
export function deriveTrialStatus(
  startedAt: string | null,
  now: Date = new Date(),
): TrialStatus {
  if (!startedAt) {
    return { state: "never-started", daysRemaining: 0, showBanner: false };
  }
  const start = Date.parse(startedAt);
  if (Number.isNaN(start)) {
    // Corrupt storage value — treat as never-started so we don't
    // strand the user in a wedged state.
    return { state: "never-started", daysRemaining: 0, showBanner: false };
  }
  const elapsedMs = now.getTime() - start;
  const dayMs = 24 * 60 * 60 * 1000;
  // Clamp elapsed at 0 so a system clock set BEFORE `startedAt`
  // doesn't grant bonus days. Clamping at the floor (not the ceiling)
  // means a clock rollback can't extend the trial past the original
  // 14 days — but also can't accidentally expire it early.
  const elapsedDays = Math.max(0, Math.floor(elapsedMs / dayMs));
  const daysRemaining = Math.max(0, TRIAL_DURATION_DAYS - elapsedDays);
  const state: TrialState = daysRemaining > 0 ? "active" : "expired";
  // Banner from day 7 onward — "trial ends in 7 days" is the first
  // user-visible reminder before the day-14 hard cutoff.
  const showBanner = state === "active" && daysRemaining <= 7;
  return { state, daysRemaining, startedAt, showBanner };
}

/** Read the persisted trial start timestamp. Returns null when
 *  storage is unavailable or no trial has begun. */
export function readTrialStartedAt(
  storage: TrialStorage | null = defaultStorage(),
): string | null {
  if (!storage) return null;
  try {
    return storage.getItem(TRIAL_STARTED_AT_KEY);
  } catch {
    return null;
  }
}

/** Mark the trial as started. No-op if already set — the trial
 *  clock only ticks forward.
 *
 *  Returns the timestamp that ended up in storage (existing value
 *  if any, freshly-written otherwise). */
export function startTrialIfUnset(
  storage: TrialStorage | null = defaultStorage(),
  now: Date = new Date(),
): string | null {
  if (!storage) return null;
  try {
    const existing = storage.getItem(TRIAL_STARTED_AT_KEY);
    if (existing) return existing;
    const iso = now.toISOString();
    storage.setItem(TRIAL_STARTED_AT_KEY, iso);
    return iso;
  } catch {
    return null;
  }
}

/** Has the user ever held a paid Pro tier? Used to suppress a fresh
 *  trial after a paid user signs out. */
export function hasEverPaid(
  storage: TrialStorage | null = defaultStorage(),
): boolean {
  if (!storage) return false;
  try {
    return storage.getItem(TRIAL_EVER_PAID_KEY) === "1";
  } catch {
    return false;
  }
}

/** Latch the "ever paid" flag. Idempotent.
 *
 *  TODO(wiring-pr): currently unreferenced — the call site lives in
 *  the auth flow (useAuthStore.setAuth) which is owned by another
 *  session. Wire from there when the tier promotes to Pro/Team/
 *  Enterprise. Keeping the helper here so the cloud-side PR is a
 *  one-liner. */
export function markEverPaid(
  storage: TrialStorage | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(TRIAL_EVER_PAID_KEY, "1");
  } catch {
    // Best-effort. If storage is rejecting writes, falling back to
    // re-arming a trial after sign-out is a smaller failure than
    // throwing here.
  }
}
