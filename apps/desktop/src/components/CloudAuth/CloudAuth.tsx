import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Cloud,
  LogIn,
  LogOut,
  User,
  Github,
  Mail,
  Lock,
  Loader2,
  AlertCircle,
  CheckCircle,
  Users,
  Bell,
  ChevronRight,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useCloudStore, initializeCloudAuth } from '@/stores/useCloudStore';
import { getGitHubAuthUrl } from '@/lib/cloud-api';

type AuthMode = 'login' | 'register';

export default function CloudAuth() {
  const { t } = useTranslation();
  const {
    user,
    isAuthenticated,
    isLoading,
    error,
    teams,
    pendingInvitations,
    login,
    register,
    logout,
    clearError,
  } = useCloudStore();

  const [mode, setMode] = useState<AuthMode>('login');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [name, setName] = useState('');
  const [showForm, setShowForm] = useState(false);

  // Initialize auth on mount
  useEffect(() => {
    initializeCloudAuth();
  }, []);

  // Handle OAuth callback (check URL params)
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const accessToken = params.get('access_token');
    const refreshToken = params.get('refresh_token');

    if (accessToken && refreshToken) {
      useCloudStore.getState().loginWithGitHub({ accessToken, refreshToken });
      // Clean URL
      window.history.replaceState({}, '', window.location.pathname);
    }
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    clearError();

    try {
      if (mode === 'login') {
        await login(email, password);
      } else {
        await register(email, password, name);
      }
      // Reset form
      setEmail('');
      setPassword('');
      setName('');
      setShowForm(false);
    } catch {
      // Error is handled in store
    }
  };

  const handleGitHubLogin = () => {
    // Open GitHub OAuth in a new window or redirect
    window.location.href = getGitHubAuthUrl();
  };

  // Authenticated view
  if (isAuthenticated && user) {
    return (
      <div className="space-y-6">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-xl font-semibold flex items-center gap-2">
              <Cloud className="text-cs-accent" size={24} />
              ATO Cloud
            </h2>
            <p className="text-sm text-cs-muted mt-1">
              Sync skills and collaborate with your team
            </p>
          </div>
        </div>

        {/* User Card */}
        <div className="card">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              {user.avatar_url ? (
                <img
                  src={user.avatar_url}
                  alt={user.name}
                  className="w-12 h-12 rounded-full"
                />
              ) : (
                <div className="w-12 h-12 rounded-full bg-cs-accent/20 flex items-center justify-center">
                  <User size={24} className="text-cs-accent" />
                </div>
              )}
              <div>
                <p className="font-medium">{user.name}</p>
                <p className="text-sm text-cs-muted">{user.email}</p>
                {user.github_username && (
                  <p className="text-xs text-cs-muted flex items-center gap-1 mt-1">
                    <Github size={12} />
                    {user.github_username}
                  </p>
                )}
              </div>
            </div>
            <button
              onClick={logout}
              className="flex items-center gap-2 px-3 py-2 text-sm text-red-400 hover:bg-red-400/10 rounded-md transition-colors"
            >
              <LogOut size={16} />
              Sign Out
            </button>
          </div>
        </div>

        {/* Stats Row */}
        <div className="grid grid-cols-2 gap-4">
          <div className="card">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-cs-accent/20">
                <Users size={20} className="text-cs-accent" />
              </div>
              <div>
                <p className="text-2xl font-semibold">{teams.length}</p>
                <p className="text-xs text-cs-muted">Teams</p>
              </div>
            </div>
          </div>
          <div className="card">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-yellow-400/20">
                <Bell size={20} className="text-yellow-400" />
              </div>
              <div>
                <p className="text-2xl font-semibold">{pendingInvitations.length}</p>
                <p className="text-xs text-cs-muted">Pending Invitations</p>
              </div>
            </div>
          </div>
        </div>

        {/* Pending Invitations */}
        {pendingInvitations.length > 0 && (
          <div className="card">
            <h3 className="text-sm font-medium text-cs-muted mb-3">Pending Invitations</h3>
            <div className="space-y-2">
              {pendingInvitations.map((invite) => (
                <div
                  key={invite.id}
                  className="flex items-center justify-between p-3 bg-cs-bg rounded-lg"
                >
                  <div>
                    <p className="font-medium">{invite.team?.name}</p>
                    <p className="text-xs text-cs-muted">
                      Invited by {invite.invited_by_user?.name} as {invite.role}
                    </p>
                  </div>
                  <ChevronRight size={16} className="text-cs-muted" />
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Teams List */}
        {teams.length > 0 && (
          <div className="card">
            <h3 className="text-sm font-medium text-cs-muted mb-3">Your Teams</h3>
            <div className="space-y-2">
              {teams.map((team) => (
                <div
                  key={team.id}
                  className="flex items-center justify-between p-3 bg-cs-bg rounded-lg hover:bg-cs-border/30 cursor-pointer transition-colors"
                >
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-lg bg-cs-accent/20 flex items-center justify-center">
                      <Users size={18} className="text-cs-accent" />
                    </div>
                    <div>
                      <p className="font-medium">{team.name}</p>
                      <p className="text-xs text-cs-muted">
                        {team.member_count} member{team.member_count !== 1 ? 's' : ''} · {team.role}
                      </p>
                    </div>
                  </div>
                  <ChevronRight size={16} className="text-cs-muted" />
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Sync Status */}
        <div className="card">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-sm font-medium">Cloud Sync</h3>
              <p className="text-xs text-cs-muted mt-1">
                Keep your skills in sync across devices
              </p>
            </div>
            <div className="flex items-center gap-2">
              <CheckCircle size={16} className="text-green-400" />
              <span className="text-sm text-green-400">Connected</span>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // Unauthenticated view
  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-xl font-semibold flex items-center gap-2">
          <Cloud className="text-cs-accent" size={24} />
          ATO Cloud
        </h2>
        <p className="text-sm text-cs-muted mt-1">
          Sign in to sync your skills and collaborate with your team
        </p>
      </div>

      {/* Benefits */}
      <div className="card">
        <h3 className="text-sm font-medium mb-4">Cloud Features</h3>
        <ul className="space-y-3">
          <li className="flex items-center gap-3 text-sm">
            <CheckCircle size={16} className="text-green-400 flex-shrink-0" />
            <span>Sync skills across all your devices</span>
          </li>
          <li className="flex items-center gap-3 text-sm">
            <CheckCircle size={16} className="text-green-400 flex-shrink-0" />
            <span>Share skills with your team</span>
          </li>
          <li className="flex items-center gap-3 text-sm">
            <CheckCircle size={16} className="text-green-400 flex-shrink-0" />
            <span>Collaborate on team skill libraries</span>
          </li>
          <li className="flex items-center gap-3 text-sm">
            <CheckCircle size={16} className="text-green-400 flex-shrink-0" />
            <span>Backup your configurations</span>
          </li>
        </ul>
      </div>

      {/* Error Display */}
      {error && (
        <div className="flex items-center gap-2 p-3 bg-red-500/10 border border-red-500/30 rounded-lg text-red-400 text-sm">
          <AlertCircle size={16} />
          {error}
        </div>
      )}

      {!showForm ? (
        // Auth Options
        <div className="space-y-3">
          <button
            onClick={handleGitHubLogin}
            className="w-full flex items-center justify-center gap-2 p-3 bg-[#24292e] hover:bg-[#2f363d] text-white rounded-lg font-medium transition-colors"
          >
            <Github size={20} />
            Continue with GitHub
          </button>

          <div className="relative">
            <div className="absolute inset-0 flex items-center">
              <div className="w-full border-t border-cs-border" />
            </div>
            <div className="relative flex justify-center text-xs">
              <span className="px-2 bg-cs-bg text-cs-muted">or</span>
            </div>
          </div>

          <button
            onClick={() => setShowForm(true)}
            className="w-full flex items-center justify-center gap-2 p-3 border border-cs-border hover:bg-cs-border/50 rounded-lg font-medium transition-colors"
          >
            <Mail size={20} />
            Continue with Email
          </button>
        </div>
      ) : (
        // Email Form
        <form onSubmit={handleSubmit} className="space-y-4">
          {/* Mode Toggle */}
          <div className="flex rounded-lg bg-cs-border/50 p-1">
            <button
              type="button"
              onClick={() => setMode('login')}
              className={cn(
                'flex-1 py-2 text-sm font-medium rounded-md transition-colors',
                mode === 'login' ? 'bg-cs-card text-cs-text' : 'text-cs-muted hover:text-cs-text'
              )}
            >
              Sign In
            </button>
            <button
              type="button"
              onClick={() => setMode('register')}
              className={cn(
                'flex-1 py-2 text-sm font-medium rounded-md transition-colors',
                mode === 'register' ? 'bg-cs-card text-cs-text' : 'text-cs-muted hover:text-cs-text'
              )}
            >
              Create Account
            </button>
          </div>

          {/* Name (register only) */}
          {mode === 'register' && (
            <div>
              <label className="block text-sm font-medium mb-1">Name</label>
              <div className="relative">
                <User size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
                <input
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Your name"
                  required
                  className="w-full pl-10 pr-4 py-2 bg-cs-card border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent"
                />
              </div>
            </div>
          )}

          {/* Email */}
          <div>
            <label className="block text-sm font-medium mb-1">Email</label>
            <div className="relative">
              <Mail size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
              <input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="you@example.com"
                required
                className="w-full pl-10 pr-4 py-2 bg-cs-card border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent"
              />
            </div>
          </div>

          {/* Password */}
          <div>
            <label className="block text-sm font-medium mb-1">Password</label>
            <div className="relative">
              <Lock size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="••••••••"
                required
                minLength={8}
                className="w-full pl-10 pr-4 py-2 bg-cs-card border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent"
              />
            </div>
            {mode === 'register' && (
              <p className="text-xs text-cs-muted mt-1">
                Min 8 characters with uppercase, lowercase, and number
              </p>
            )}
          </div>

          {/* Submit */}
          <button
            type="submit"
            disabled={isLoading}
            className="w-full flex items-center justify-center gap-2 p-3 bg-cs-accent hover:bg-cs-accent/90 text-cs-bg rounded-lg font-medium transition-colors disabled:opacity-50"
          >
            {isLoading ? (
              <Loader2 size={20} className="animate-spin" />
            ) : (
              <LogIn size={20} />
            )}
            {mode === 'login' ? 'Sign In' : 'Create Account'}
          </button>

          {/* Back to options */}
          <button
            type="button"
            onClick={() => setShowForm(false)}
            className="w-full text-sm text-cs-muted hover:text-cs-text transition-colors"
          >
            Back to all options
          </button>
        </form>
      )}
    </div>
  );
}
