import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import {
  CloudUser,
  AuthTokens,
  Team,
  TeamInvitation,
  login as apiLogin,
  register as apiRegister,
  getCurrentUser,
  getTeams,
  getPendingInvitations,
  storeTokens,
  clearTokens,
  getStoredTokens,
} from '@/lib/cloud-api';

interface CloudState {
  // Auth state
  user: CloudUser | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  error: string | null;

  // Teams state
  teams: Team[];
  pendingInvitations: TeamInvitation[];
  selectedTeamId: string | null;

  // Sync state
  syncEnabled: boolean;
  lastSyncAt: string | null;

  // Actions
  login: (email: string, password: string) => Promise<void>;
  register: (email: string, password: string, name: string) => Promise<void>;
  loginWithGitHub: (tokens: AuthTokens) => Promise<void>;
  logout: () => void;
  refreshUser: () => Promise<void>;
  fetchTeams: () => Promise<void>;
  fetchPendingInvitations: () => Promise<void>;
  selectTeam: (teamId: string | null) => void;
  setSyncEnabled: (enabled: boolean) => void;
  clearError: () => void;
}

export const useCloudStore = create<CloudState>()(
  persist(
    (set, get) => ({
      // Initial state
      user: null,
      isAuthenticated: false,
      isLoading: false,
      error: null,
      teams: [],
      pendingInvitations: [],
      selectedTeamId: null,
      syncEnabled: false,
      lastSyncAt: null,

      // Login with email/password
      login: async (email: string, password: string) => {
        set({ isLoading: true, error: null });
        try {
          const response = await apiLogin(email, password);
          storeTokens(response.tokens);
          set({
            user: response.user,
            isAuthenticated: true,
            isLoading: false,
          });
          // Fetch teams after login
          await get().fetchTeams();
          await get().fetchPendingInvitations();
        } catch (err) {
          set({
            isLoading: false,
            error: err instanceof Error ? err.message : 'Login failed',
          });
          throw err;
        }
      },

      // Register new account
      register: async (email: string, password: string, name: string) => {
        set({ isLoading: true, error: null });
        try {
          const response = await apiRegister(email, password, name);
          storeTokens(response.tokens);
          set({
            user: response.user,
            isAuthenticated: true,
            isLoading: false,
          });
        } catch (err) {
          set({
            isLoading: false,
            error: err instanceof Error ? err.message : 'Registration failed',
          });
          throw err;
        }
      },

      // Login with GitHub OAuth (tokens received from callback)
      loginWithGitHub: async (tokens: AuthTokens) => {
        set({ isLoading: true, error: null });
        try {
          storeTokens(tokens);
          const { user } = await getCurrentUser();
          set({
            user,
            isAuthenticated: true,
            isLoading: false,
          });
          // Fetch teams after login
          await get().fetchTeams();
          await get().fetchPendingInvitations();
        } catch (err) {
          clearTokens();
          set({
            isLoading: false,
            error: err instanceof Error ? err.message : 'GitHub login failed',
          });
          throw err;
        }
      },

      // Logout
      logout: () => {
        clearTokens();
        set({
          user: null,
          isAuthenticated: false,
          teams: [],
          pendingInvitations: [],
          selectedTeamId: null,
          error: null,
        });
      },

      // Refresh user data
      refreshUser: async () => {
        const tokens = getStoredTokens();
        if (!tokens) {
          set({ isAuthenticated: false, user: null });
          return;
        }

        try {
          const { user } = await getCurrentUser();
          set({ user, isAuthenticated: true });
        } catch {
          // Token might be expired, try to use stored state
          set({ isAuthenticated: false, user: null });
          clearTokens();
        }
      },

      // Fetch user's teams
      fetchTeams: async () => {
        try {
          const teams = await getTeams();
          set({ teams });
        } catch (err) {
          console.error('Failed to fetch teams:', err);
        }
      },

      // Fetch pending invitations
      fetchPendingInvitations: async () => {
        try {
          const invitations = await getPendingInvitations();
          set({ pendingInvitations: invitations });
        } catch (err) {
          console.error('Failed to fetch invitations:', err);
        }
      },

      // Select active team
      selectTeam: (teamId: string | null) => {
        set({ selectedTeamId: teamId });
      },

      // Toggle sync
      setSyncEnabled: (enabled: boolean) => {
        set({ syncEnabled: enabled });
      },

      // Clear error
      clearError: () => {
        set({ error: null });
      },
    }),
    {
      name: 'ato-cloud-store',
      partialize: (state) => ({
        syncEnabled: state.syncEnabled,
        selectedTeamId: state.selectedTeamId,
      }),
    }
  )
);

// Initialize auth state on app load
export async function initializeCloudAuth() {
  const tokens = getStoredTokens();
  if (tokens) {
    try {
      const { user } = await getCurrentUser();
      useCloudStore.setState({ user, isAuthenticated: true });
      // Fetch teams in background
      useCloudStore.getState().fetchTeams();
      useCloudStore.getState().fetchPendingInvitations();
    } catch {
      // Token invalid, clear it
      clearTokens();
      useCloudStore.setState({ user: null, isAuthenticated: false });
    }
  }
}
