import { create } from "zustand";
import { persist } from "zustand/middleware";
import { refreshToken as refreshTokenApi } from "@/lib/api";

interface User {
  id: string;
  name: string;
  email: string;
}

interface AuthState {
  user: User | null;
  accessToken: string | null;
  refreshTokenValue: string | null;
  isAuthenticated: boolean;

  setAuth: (user: User, accessToken: string, refreshToken: string) => void;
  logout: () => void;
  refreshAccessToken: () => Promise<boolean>;
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

      setAuth: (user, accessToken, refreshToken) =>
        set({
          user,
          accessToken,
          refreshTokenValue: refreshToken,
          isAuthenticated: true,
        }),

      logout: () =>
        set({
          user: localUser,
          accessToken: null,
          refreshTokenValue: null,
          isAuthenticated: true, // stays true — local mode
        }),

      refreshAccessToken: async () => {
        const { refreshTokenValue } = get();
        if (!refreshTokenValue) return true; // local mode, always ok
        try {
          const result = await refreshTokenApi(refreshTokenValue);
          set({ accessToken: result.accessToken });
          return true;
        } catch {
          return true; // don't break local mode
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
      }),
    }
  )
);
