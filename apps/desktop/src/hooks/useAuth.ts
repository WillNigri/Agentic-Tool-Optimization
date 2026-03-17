import { create } from "zustand";
import { persist } from "zustand/middleware";
import { refreshToken as refreshTokenApi } from "@/lib/api";

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

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

// In Tauri desktop mode, user is always "authenticated" (local-first, no login)
const localUser: User = { id: 'local', name: 'Local User', email: '' };

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      // Desktop app starts authenticated; web mode requires login
      user: isTauri ? localUser : null,
      accessToken: null,
      refreshTokenValue: null,
      isAuthenticated: isTauri ? true : false,

      setAuth: (user, accessToken, refreshToken) =>
        set({
          user,
          accessToken,
          refreshTokenValue: refreshToken,
          isAuthenticated: true,
        }),

      logout: () => {
        if (isTauri) return; // Can't log out of desktop app
        set({
          user: null,
          accessToken: null,
          refreshTokenValue: null,
          isAuthenticated: false,
        });
      },

      refreshAccessToken: async () => {
        if (isTauri) return true; // Desktop doesn't use tokens
        const { refreshTokenValue } = get();
        if (!refreshTokenValue) return false;

        try {
          const result = await refreshTokenApi(refreshTokenValue);
          set({ accessToken: result.accessToken });
          return true;
        } catch {
          get().logout();
          return false;
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
