import { create } from "zustand";
import { persist } from "zustand/middleware";
import { refreshToken as refreshTokenApi } from "@/lib/api";
import { getCurrentUser, storeTokens, clearTokens } from "@/lib/cloud-api";
import { markEverPaid, startTrialIfUnset, hasEverPaid } from "@/lib/trial";

// Phase 1 PR-B (2026-05-21) — `latchEverPaidIfPaid` runs at every store
// transition that can promote the user's tier (setAuth, setTier,
// refreshTier). It calls markEverPaid() iff the new tier is paid
// (pro / team / enterprise). The latch is monotonic + idempotent
// (writes "1" to localStorage), so over-firing is a no-op. Under-firing
// would leave the trial-re-arm loophole open — a paid user signs out
// and gets a fresh 14-day trial. Defense in depth at near-zero cost.
function latchEverPaidIfPaid(tier: Tier): void {
  if (tier !== "free") markEverPaid();
}

interface User {
  id: string;
  name: string;
  email: string;
}

/** v1.4.0 — Subscription tier. Mirrors the cloud `users.subscription_tier`
 *  column (migration 007). Local-only users are 'free'. */
export type Tier = "free" | "pro" | "team" | "enterprise";

interface AuthState {
  user: User | null;
  accessToken: string | null;
  refreshTokenValue: string | null;
  isAuthenticated: boolean;
  isCloudUser: boolean;
  /** Subscription tier. Defaults to 'free'; updated to the cloud value when
   *  the user signs in. Used by `lib/tier.ts` to gate Pro+ features. */
  tier: Tier;

  setAuth: (user: User, accessToken: string, refreshToken: string, tier?: Tier) => void;
  setTier: (tier: Tier) => void;
  logout: () => void;
  refreshAccessToken: () => Promise<boolean>;
  /** Re-fetch /auth/me and pull the latest subscription_tier into the store.
   *  Best-effort — silently no-ops if the user isn't a cloud user, the
   *  network is down, or the access token is stale. */
  refreshTier: () => Promise<void>;
}

// Desktop app is local-first: always authenticated.
// Cloud login is only needed when sync is enabled (handled separately).
const localUser: User = { id: 'local', name: 'Local User', email: '' };

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      user: localUser,
      accessToken: null,
      refreshTokenValue: null,
      isAuthenticated: true,
      isCloudUser: false,
      tier: "free",

      setAuth: (user, accessToken, refreshToken, tier) => {
        // Mirror tokens into the localStorage slot that `lib/cloud-api.ts`
        // reads via `getStoredTokens()`. Without this, anything routed
        // through `cloudApi*` (Deploy tab embed key, agent-suggest,
        // backups) fails with "Not authenticated" even though zustand
        // says we're signed in. Two stores; one truth.
        storeTokens({ accessToken, refreshToken });
        const resolvedTier = tier ?? "pro"; // signed-in cloud users default to pro until /auth/me reports otherwise
        set({
          user,
          accessToken,
          refreshTokenValue: refreshToken,
          isAuthenticated: true,
          isCloudUser: true,
          tier: resolvedTier,
        });
        latchEverPaidIfPaid(resolvedTier);
        // 2026-05-27 — start the 14-day trial clock on cloud login if
        // it hasn't started yet AND the user has never paid (paid
        // users don't get a fresh trial after sign-out). Without this,
        // a brand-new signup sees no trial banner until they first
        // touch a Pro feature, which means no Stripe Subscribe button
        // is visible either — caught live by Will on willnigri+4.
        if (!hasEverPaid()) {
          startTrialIfUnset();
        }
        // Backfill local traces to cloud on login (non-blocking).
        // Gives day-1 analytics for new Pro users and fills gaps
        // from periods when the user was logged out.
        import("@/lib/traceBackfill").then(({ backfillLocalTraces }) => {
          backfillLocalTraces().catch(() => {});
        });
      },

      setTier: (tier) => {
        set({ tier });
        latchEverPaidIfPaid(tier);
      },

      logout: () => {
        clearTokens(); // wipe the localStorage mirror so cloud-api stops sending stale Bearer
        // Model A: scrub the cached cloud member id so post-logout local
        // dispatches aren't mis-attributed to the previous user. This is the
        // chokepoint every logout path funnels through — including the
        // 401/403 paths in agentTraceUpload/cloudAgentTraces that bypass
        // syncToAuthStore. Fire-and-forget + Tauri-guarded.
        import("@tauri-apps/api/core")
          .then(({ invoke }) => invoke("set_local_member_id", { memberId: null }))
          .catch(() => {});
        set({
          user: localUser,
          accessToken: null,
          refreshTokenValue: null,
          isAuthenticated: true, // stays true — local mode
          isCloudUser: false,
          tier: "free",
        });
      },

      refreshAccessToken: async () => {
        const { refreshTokenValue } = get();
        if (!refreshTokenValue) return true; // local mode, always ok
        try {
          const result = await refreshTokenApi(refreshTokenValue);
          set({ accessToken: result.accessToken });
          // Keep the cloud-api localStorage mirror in sync after rotation.
          storeTokens({ accessToken: result.accessToken, refreshToken: refreshTokenValue });
          return true;
        } catch {
          return true; // don't break local mode
        }
      },

      refreshTier: async () => {
        const { isCloudUser, accessToken } = get();
        if (!isCloudUser || !accessToken) return;
        try {
          const { user } = await getCurrentUser();
          if (user.subscription_tier) {
            const newTier = user.subscription_tier as Tier;
            set({ tier: newTier });
            latchEverPaidIfPaid(newTier);
          }
        } catch {
          // Silent — keep the cached tier on offline / 401.
        }
      },
    }),
    {
      name: "ato-auth",
      partialize: (state) => ({
        user: state.user,
        accessToken: state.accessToken,
        refreshTokenValue: state.refreshTokenValue,
        isAuthenticated: state.isAuthenticated,
        // Without persisting isCloudUser, every reload reset it to false
        // even though tokens were intact — Pro gates then showed
        // "Sign in for Pro" despite a valid session. Beatriz hit this
        // 2026-05-09 right after the localStorage-mirror fix.
        isCloudUser: state.isCloudUser,
        tier: state.tier,
      }),
      // Mirror persisted tokens into the legacy localStorage slot that
      // lib/cloud-api.ts reads via getStoredTokens(). Without this, an
      // existing session predating the setAuth-mirror fix rehydrates
      // with tokens in zustand but nothing in localStorage, and the
      // Deploy-tab embed key path keeps 401-ing.
      onRehydrateStorage: () => (state) => {
        if (state?.accessToken && state?.refreshTokenValue) {
          storeTokens({
            accessToken: state.accessToken,
            refreshToken: state.refreshTokenValue,
          });
        }
      },
    }
  )
);
