import { useState, useEffect } from 'react';
import { useAuthStore } from '@/hooks/useAuth';
import Sidebar, { type Section } from '@/components/Sidebar';
import UsageAnalytics from '@/components/UsageAnalytics';
import AuditLog from '@/components/AuditLog/AuditLog';
import LlmApiKeys from '@/components/LlmApiKeys/LlmApiKeys';
import AgentMonitor from '@/components/AgentMonitor/AgentMonitor';
import CostDashboard from './CostDashboard';
import LoginModal from '@/components/LoginModal';

const PANELS: Partial<Record<Section, React.ComponentType>> = {
  analytics: UsageAnalytics,
  'agent-monitor': AgentMonitor,
  'llm-keys': LlmApiKeys,
  audit: AuditLog,
};

export default function WebDashboard() {
  const [section, setSection] = useState<Section>('analytics');
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const user = useAuthStore((s) => s.user);
  const [showLogin, setShowLogin] = useState(false);

  // Check for OAuth callback tokens in URL
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const accessToken = params.get('access_token');
    const refreshToken = params.get('refresh_token');
    if (accessToken && refreshToken) {
      useAuthStore.getState().setTokens(accessToken, refreshToken);
      const userId = params.get('user_id');
      const email = params.get('user_email');
      const name = params.get('user_name');
      if (userId && email) {
        useAuthStore.getState().setUser({ id: userId, email, name: name || email });
      }
      // Clean URL
      window.history.replaceState({}, '', window.location.pathname);
    }
  }, []);

  if (!isCloudUser) {
    return (
      <div className="min-h-screen bg-[#0a0a0f] flex items-center justify-center">
        <div className="text-center max-w-md mx-auto px-6">
          <h1 className="text-3xl font-bold text-white mb-2">ATO Dashboard</h1>
          <p className="text-gray-400 mb-8">
            Sign in to access your team's LLM analytics, agent monitoring, and cost tracking.
          </p>
          <button
            onClick={() => setShowLogin(true)}
            className="px-6 py-3 bg-[#00FFB2] text-black font-semibold rounded-md hover:bg-[#00FFB2]/90 transition-colors"
          >
            Sign In
          </button>
          <p className="text-gray-500 text-sm mt-4">
            Don't have an account?{' '}
            <a href="https://agentictool.ai" className="text-[#00FFB2] hover:underline">
              Download ATO
            </a>{' '}
            — it's free and open source.
          </p>
          {showLogin && <LoginModal onClose={() => setShowLogin(false)} />}
        </div>
      </div>
    );
  }

  const Panel = PANELS[section] || CostDashboard;

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar active={section} onNavigate={setSection} />
      <main className="flex-1 overflow-y-auto p-6">
        <Panel />
      </main>
    </div>
  );
}
