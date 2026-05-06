import { useState, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  BarChart3,
  Key,
  Settings,
  Shield,
  Activity,
  LogOut,
  Crown,
} from 'lucide-react';
import CostDashboard from './CostDashboard';
import ApiKeysPanel from './ApiKeysPanel';
import Onboarding from './Onboarding';

const API_BASE = import.meta.env.VITE_API_URL || 'https://api.agentictool.ai/api';

// Simple auth store using localStorage
function useAuth() {
  const [user, setUser] = useState<{ id: string; email: string; name: string } | null>(null);
  const [token, setToken] = useState<string | null>(null);

  useEffect(() => {
    // Check for OAuth callback tokens in URL
    const params = new URLSearchParams(window.location.search);
    const accessToken = params.get('access_token');
    const refreshToken = params.get('refresh_token');
    if (accessToken && refreshToken) {
      const userId = params.get('user_id') || '';
      const email = params.get('user_email') || '';
      const name = params.get('user_name') || email;
      setToken(accessToken);
      setUser({ id: userId, email, name });
      localStorage.setItem('ato-auth', JSON.stringify({
        state: { accessToken, refreshToken, user: { id: userId, email, name } }
      }));
      window.history.replaceState({}, '', window.location.pathname);
      return;
    }

    // Load from localStorage
    const stored = localStorage.getItem('ato-auth');
    if (stored) {
      try {
        const { state } = JSON.parse(stored);
        if (state?.accessToken) {
          setToken(state.accessToken);
          setUser(state.user);
        }
      } catch { /* ignore */ }
    }
  }, []);

  const login = async (email: string, password: string) => {
    const res = await fetch(`${API_BASE}/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password }),
    });
    const json = await res.json();
    if (!json.success) throw new Error(json.error?.message || 'Login failed');
    setToken(json.data.tokens.accessToken);
    setUser(json.data.user);
    localStorage.setItem('ato-auth', JSON.stringify({
      state: { accessToken: json.data.tokens.accessToken, refreshToken: json.data.tokens.refreshToken, user: json.data.user }
    }));
    return json.data;
  };

  const register = async (email: string, password: string, name: string) => {
    const res = await fetch(`${API_BASE}/auth/register`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password, name }),
    });
    const json = await res.json();
    if (!json.success) throw new Error(json.error?.message || 'Registration failed');
    setToken(json.data.tokens.accessToken);
    setUser(json.data.user);
    localStorage.setItem('ato-auth', JSON.stringify({
      state: { accessToken: json.data.tokens.accessToken, refreshToken: json.data.tokens.refreshToken, user: json.data.user }
    }));
    return json.data;
  };

  const logout = () => {
    setToken(null);
    setUser(null);
    localStorage.removeItem('ato-auth');
    localStorage.removeItem('ato-onboarding-complete');
  };

  const loginWithGithub = () => {
    window.location.href = `${API_BASE}/auth/github?redirect=${encodeURIComponent(window.location.origin + '/auth/callback')}`;
  };

  return { user, token, login, register, logout, loginWithGithub, isAuthenticated: !!token };
}

type Panel = 'costs' | 'api-keys' | 'settings';

const NAV_ITEMS: { id: Panel; label: string; icon: typeof BarChart3 }[] = [
  { id: 'costs', label: 'Cost Dashboard', icon: BarChart3 },
  { id: 'api-keys', label: 'API Keys', icon: Key },
];

export default function WebDashboard() {
  const { user, token, login, register, logout, loginWithGithub, isAuthenticated } = useAuth();
  const [panel, setPanel] = useState<Panel>('costs');
  const [showOnboarding, setShowOnboarding] = useState(false);

  // Show onboarding for new users who haven't completed it
  useEffect(() => {
    if (isAuthenticated && !localStorage.getItem('ato-onboarding-complete')) {
      setShowOnboarding(true);
    }
  }, [isAuthenticated]);

  if (!isAuthenticated) {
    return <LoginPage onLogin={login} onRegister={register} onGithub={loginWithGithub} />;
  }

  if (showOnboarding) {
    return <Onboarding onComplete={() => setShowOnboarding(false)} />;
  }

  const PanelComponent = panel === 'api-keys' ? ApiKeysPanel : CostDashboard;

  return (
    <div className="flex h-screen bg-[#0a0a0f]">
      {/* Sidebar */}
      <aside className="w-56 h-screen bg-[#16161e] border-r border-[#2a2a3a] flex flex-col shrink-0">
        <div className="px-4 py-5 border-b border-[#2a2a3a]">
          <h1 className="text-lg font-bold text-white tracking-tight">ATO</h1>
          <p className="text-xs text-[#8888a0] mt-0.5 truncate">{user?.email}</p>
        </div>

        <nav className="flex-1 py-3 px-2 space-y-0.5">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                onClick={() => setPanel(item.id)}
                className={`w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
                  panel === item.id
                    ? 'bg-[#00FFB2]/15 text-[#00FFB2]'
                    : 'text-[#8888a0] hover:text-white hover:bg-[#2a2a3a]/50'
                }`}
              >
                <Icon size={18} />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="p-2 border-t border-[#2a2a3a]">
          <div className="flex items-center gap-2 px-3 py-2">
            <div className="w-7 h-7 rounded-full bg-[#00FFB2]/10 border border-[#00FFB2]/30 flex items-center justify-center">
              <Crown size={14} className="text-[#00FFB2]" />
            </div>
            <div className="min-w-0">
              <p className="text-xs font-medium text-white truncate">{user?.name || user?.email}</p>
            </div>
          </div>
          <button
            onClick={logout}
            className="w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm text-[#8888a0] hover:text-red-400 hover:bg-red-500/10 transition-colors"
          >
            <LogOut size={18} />
            Logout
          </button>
        </div>
      </aside>

      {/* Main */}
      <main className="flex-1 overflow-y-auto p-6">
        <PanelComponent />
      </main>
    </div>
  );
}

function LoginPage({ onLogin, onRegister, onGithub }: {
  onLogin: (email: string, password: string) => Promise<any>;
  onRegister: (email: string, password: string, name: string) => Promise<any>;
  onGithub: () => void;
}) {
  const [mode, setMode] = useState<'login' | 'register'>('login');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [name, setName] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      if (mode === 'register') {
        await onRegister(email, password, name);
      } else {
        await onLogin(email, password);
      }
    } catch (err: any) {
      setError(err.message);
    }
    setLoading(false);
  };

  return (
    <div className="min-h-screen bg-[#0a0a0f] text-white">
      {/* Marketing hero — shown to anyone hitting the page unauthenticated */}
      <section className="max-w-5xl mx-auto px-6 pt-16 pb-10 text-center">
        <h1 className="text-4xl sm:text-5xl font-bold tracking-tight">
          The GUI for daily <span className="text-[#00FFB2]">agentic work</span>.
        </h1>
        <p className="mt-4 text-base sm:text-lg text-[#aaaab8] max-w-2xl mx-auto">
          Persistent multi-runtime conversations, production-grade agent authoring, and
          observability — across Claude Code, Codex, Gemini, OpenClaw, Hermes, and Ollama.
        </p>
        <p className="mt-2 text-sm text-[#8888a0] max-w-2xl mx-auto">
          Switch from Claude to Codex mid-conversation. Threads persist. Tokens stream.
          Markdown renders. Local-first. MIT licensed.
        </p>

        <div className="mt-7 flex flex-wrap items-center justify-center gap-3">
          <a
            href="https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest"
            className="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 transition-colors"
          >
            Download desktop app
          </a>
          <a
            href="https://github.com/WillNigri/Agentic-Tool-Optimization"
            className="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg border border-[#2a2a3a] text-sm font-medium text-white hover:bg-[#16161e] transition-colors"
          >
            View on GitHub
          </a>
        </div>

        <ul className="mt-10 grid grid-cols-1 sm:grid-cols-3 gap-3 text-left text-sm">
          <li className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-4">
            <div className="text-[#00FFB2] font-semibold">Daily workspace</div>
            <p className="mt-1 text-[#aaaab8] text-xs leading-relaxed">
              Persistent threads survive restart. Switch runtimes mid-conversation; the
              full history travels. Streaming responses with markdown + syntax highlighting.
              Drag-drop file attachments.
            </p>
          </li>
          <li className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-4">
            <div className="text-[#00FFB2] font-semibold">Production-grade authoring</div>
            <p className="mt-1 text-[#aaaab8] text-xs leading-relaxed">
              Variables, pre-call hooks, summarizers, multi-agent groups with router,
              per-task models, evaluators, observability — every context-engineering
              primitive as a first-class UI.
            </p>
          </li>
          <li className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-4">
            <div className="text-[#00FFB2] font-semibold">Multi-runtime by protocol</div>
            <p className="mt-1 text-[#aaaab8] text-xs leading-relaxed">
              Cross-runtime dispatch via MCP — any runtime can call any agent. Bring
              your own auth: ride your CLI subscriptions, or use stored API keys. Your
              choice, per runtime.
            </p>
          </li>
        </ul>
      </section>

      {/* Sign-in card */}
      <div className="flex items-start justify-center px-6 pb-16">
        <div className="max-w-sm w-full space-y-6 rounded-xl border border-[#2a2a3a] bg-[#0f0f17] p-6">
          <div className="text-center">
            <p className="text-[#8888a0]">
              {mode === 'login' ? 'Sign in to your dashboard' : 'Create your account'}
            </p>
          </div>

        <button
          onClick={onGithub}
          className="w-full flex items-center justify-center gap-2 px-4 py-3 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-white text-sm hover:bg-[#2a2a3a] transition-colors"
        >
          <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24"><path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/></svg>
          Continue with GitHub
        </button>

        <div className="flex items-center gap-3">
          <div className="flex-1 h-px bg-[#2a2a3a]" />
          <span className="text-xs text-[#8888a0]">or</span>
          <div className="flex-1 h-px bg-[#2a2a3a]" />
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          {mode === 'register' && (
            <input
              type="text"
              placeholder="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="w-full px-4 py-3 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-white text-sm focus:outline-none focus:border-[#00FFB2]/50"
              required
            />
          )}
          <input
            type="email"
            placeholder="Email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="w-full px-4 py-3 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-white text-sm focus:outline-none focus:border-[#00FFB2]/50"
            required
          />
          <input
            type="password"
            placeholder="Password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full px-4 py-3 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-white text-sm focus:outline-none focus:border-[#00FFB2]/50"
            required
          />
          {error && <p className="text-red-400 text-sm">{error}</p>}
          <button
            type="submit"
            disabled={loading}
            className="w-full px-6 py-3 bg-[#00FFB2] text-black font-semibold rounded-lg hover:bg-[#00FFB2]/90 disabled:opacity-50 transition-colors"
          >
            {loading ? '...' : mode === 'login' ? 'Sign In' : 'Create Account'}
          </button>
        </form>

          <p className="text-center text-sm text-[#8888a0]">
            {mode === 'login' ? (
              <>Don't have an account? <button onClick={() => setMode('register')} className="text-[#00FFB2] hover:underline">Sign up</button></>
            ) : (
              <>Already have an account? <button onClick={() => setMode('login')} className="text-[#00FFB2] hover:underline">Sign in</button></>
            )}
          </p>
        </div>
      </div>
    </div>
  );
}
