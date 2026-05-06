import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Key, Plus, Trash2, Copy, Check, Clock, Hash } from 'lucide-react';

const API_BASE = import.meta.env.VITE_API_URL || 'https://api.agentictool.ai/api';

function getAuthHeaders(): Record<string, string> {
  const stored = localStorage.getItem('ato-auth');
  if (!stored) return {};
  try {
    const { state } = JSON.parse(stored);
    if (state?.accessToken) return { Authorization: `Bearer ${state.accessToken}` };
  } catch { /* ignore */ }
  return {};
}

async function fetchApi<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: { 'Content-Type': 'application/json', ...getAuthHeaders(), ...options?.headers },
  });
  const json = await res.json();
  if (!json.success) throw new Error(json.error?.message || 'API error');
  return json.data;
}

interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  scopes: string[];
  last_used_at: string | null;
  usage_count: number;
  is_active: boolean;
  created_at: string;
}

function formatTimeAgo(dateStr: string | null): string {
  if (!dateStr) return 'never';
  const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

export default function ApiKeysPanel() {
  const queryClient = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [newKeyName, setNewKeyName] = useState('');
  const [newKeyValue, setNewKeyValue] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const { data: keys = [], isLoading } = useQuery({
    queryKey: ['api-keys'],
    queryFn: () => fetchApi<ApiKey[]>('/auth/api-keys'),
  });

  const createMutation = useMutation({
    mutationFn: async (name: string) => {
      return fetchApi<ApiKey & { key: string }>('/auth/api-keys', {
        method: 'POST',
        body: JSON.stringify({ name }),
      });
    },
    onSuccess: (data) => {
      setNewKeyValue(data.key);
      queryClient.invalidateQueries({ queryKey: ['api-keys'] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => fetchApi(`/auth/api-keys/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['api-keys'] }),
  });

  const copyKey = (value: string) => {
    navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold text-white flex items-center gap-2">
            <Key className="w-5 h-5 text-[#00FFB2]" />
            API Keys
          </h2>
          <p className="text-[#8888a0] text-sm">
            Keys for the @ato-sdk/js to send traces to your dashboard
          </p>
        </div>
        <button
          onClick={() => { setShowCreate(true); setNewKeyValue(null); setNewKeyName(''); }}
          className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-[#00FFB2] text-black font-medium hover:bg-[#00FFB2]/90 transition-colors"
        >
          <Plus className="w-3.5 h-3.5" />
          New Key
        </button>
      </div>

      {/* Create Key Form */}
      {showCreate && (
        <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-5 space-y-4">
          {newKeyValue ? (
            <>
              <p className="text-sm text-white font-medium">Your new API key</p>
              <p className="text-xs text-[#8888a0]">Copy it now — you won't see it again.</p>
              <div className="flex items-center gap-2 bg-[#0a0a0f] rounded-md p-3">
                <code className="text-[#00FFB2] text-sm font-mono flex-1 break-all">{newKeyValue}</code>
                <button onClick={() => copyKey(newKeyValue)} className="p-1.5 rounded hover:bg-[#2a2a3a]">
                  {copied ? <Check className="w-4 h-4 text-[#00FFB2]" /> : <Copy className="w-4 h-4 text-[#8888a0]" />}
                </button>
              </div>
              <div className="bg-[#0a0a0f] rounded-md p-3 font-mono text-sm text-[#e8e8f0]">
                <span className="text-[#8888a0]">// Add to your code:</span><br />
                init({'{'} apiKey: '{newKeyValue.slice(0, 15)}...' {'}'})
              </div>
              <button
                onClick={() => { setShowCreate(false); setNewKeyValue(null); }}
                className="px-4 py-2 text-sm rounded-md bg-[#2a2a3a] text-white hover:bg-[#32324a] transition-colors"
              >
                Done
              </button>
            </>
          ) : (
            <>
              <input
                type="text"
                placeholder="Key name (e.g. Production, CI/CD)"
                value={newKeyName}
                onChange={(e) => setNewKeyName(e.target.value)}
                className="w-full px-3 py-2 bg-[#0a0a0f] border border-[#2a2a3a] rounded-md text-white text-sm focus:outline-none focus:border-[#00FFB2]/50"
              />
              <div className="flex gap-2">
                <button
                  onClick={() => createMutation.mutate(newKeyName || 'Default')}
                  disabled={createMutation.isPending}
                  className="px-4 py-2 text-sm rounded-md bg-[#00FFB2] text-black font-medium disabled:opacity-50"
                >
                  {createMutation.isPending ? 'Creating...' : 'Create Key'}
                </button>
                <button
                  onClick={() => setShowCreate(false)}
                  className="px-4 py-2 text-sm rounded-md bg-[#2a2a3a] text-white hover:bg-[#32324a]"
                >
                  Cancel
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {/* Keys List */}
      {isLoading ? (
        <div className="space-y-2 animate-pulse">
          {[1, 2].map((i) => <div key={i} className="bg-[#16161e] h-16 rounded-lg" />)}
        </div>
      ) : keys.length === 0 ? (
        <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center py-12">
          <Key className="w-8 h-8 text-[#8888a0] mx-auto mb-3" />
          <p className="text-[#8888a0]">No API keys yet</p>
          <p className="text-[#8888a0] text-xs mt-1">Create one to start sending traces from the SDK</p>
        </div>
      ) : (
        <div className="space-y-2">
          {keys.map((key) => (
            <div key={key.id} className="bg-[#16161e] border border-[#2a2a3a] rounded-lg px-4 py-3 flex items-center gap-3 hover:border-[#00FFB2]/20 transition-colors">
              <Key className="w-4 h-4 text-[#8888a0] shrink-0" />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-white">{key.name}</span>
                  <code className="text-xs text-[#8888a0] font-mono">{key.key_prefix}...</code>
                </div>
                <div className="flex items-center gap-3 mt-1 text-xs text-[#8888a0]">
                  <span className="flex items-center gap-1"><Hash className="w-3 h-3" />{key.usage_count} uses</span>
                  <span className="flex items-center gap-1"><Clock className="w-3 h-3" />Last used {formatTimeAgo(key.last_used_at)}</span>
                </div>
              </div>
              <button
                onClick={() => { if (confirm(`Delete "${key.name}"?`)) deleteMutation.mutate(key.id); }}
                className="p-1.5 rounded hover:bg-red-500/20 text-red-400 transition-colors"
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
