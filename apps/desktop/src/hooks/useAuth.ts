import { create } from "zustand";
import { persist } from "zustand/middleware";
import { refreshToken as refreshTokenApi } from "@/lib/api";
import { getCurrentUser, storeTokens, clearTokens } from "@/lib/cloud-api";

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
        set({
          user,
          accessToken,
          refreshTokenValue: refreshToken,
          isAuthenticated: true,
          isCloudUser: true,
          tier: tier ?? "pro", // signed-in cloud users default to pro until /auth/me reports otherwise
        });
      },

      setTier: (tier) => set({ tier }),

      logout: () => {
        clearTokens(); // wipe the localStorage mirror so cloud-api stops sending stale Bearer
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
            set({ tier: user.subscription_tier as Tier });
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
