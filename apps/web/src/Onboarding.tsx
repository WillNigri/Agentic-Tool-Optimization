import { useState } from 'react';
import { Copy, Check, Key, Terminal, ChevronRight, ExternalLink } from 'lucide-react';

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

export default function Onboarding({ onComplete }: { onComplete: () => void }) {
  const [step, setStep] = useState(0);
  const [apiKey, setApiKey] = useState<string | null>(null);
  const [keyName, setKeyName] = useState('My First Key');
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');

  const createKey = async () => {
    setLoading(true);
    setError('');
    try {
      const res = await fetch(`${API_BASE}/auth/api-keys`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...getAuthHeaders() },
        body: JSON.stringify({ name: keyName }),
      });
      const json = await res.json();
      if (json.success) {
        setApiKey(json.data.key);
        setStep(1);
      } else {
        setError(json.error?.message || 'Failed to create key');
      }
    } catch (e: any) {
      setError(e.message || 'Network error');
    }
    setLoading(false);
  };

  const copyKey = () => {
    if (apiKey) {
      navigator.clipboard.writeText(apiKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  return (
    <div className="min-h-screen bg-[#0a0a0f] flex items-center justify-center p-6">
      <div className="max-w-lg w-full space-y-8">
        {/* Progress */}
        <div className="flex items-center gap-2 justify-center">
          {[0, 1, 2].map((i) => (
            <div
              key={i}
              className={`h-1 w-16 rounded-full transition-colors ${
                i <= step ? 'bg-[#00FFB2]' : 'bg-[#2a2a3a]'
              }`}
            />
          ))}
        </div>

        {/* Step 0: Create API Key */}
        {step === 0 && (
          <div className="text-center space-y-6">
            <div className="w-12 h-12 rounded-xl bg-[#00FFB2]/10 flex items-center justify-center mx-auto">
              <Key className="w-6 h-6 text-[#00FFB2]" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-white">Create your API key</h1>
              <p className="text-[#8888a0] mt-2">
                This key lets the SDK send traces to your dashboard. You'll only see it once.
              </p>
            </div>
            <div>
              <input
                type="text"
                value={keyName}
                onChange={(e) => setKeyName(e.target.value)}
                placeholder="Key name (e.g. Production, Staging)"
                className="w-full px-4 py-3 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-white text-sm focus:outline-none focus:border-[#00FFB2]/50"
              />
            </div>
            {error && <p className="text-red-400 text-sm">{error}</p>}
            <button
              onClick={createKey}
              disabled={loading}
              className="w-full px-6 py-3 bg-[#00FFB2] text-black font-semibold rounded-lg hover:bg-[#00FFB2]/90 disabled:opacity-50 transition-colors"
            >
              {loading ? 'Creating...' : 'Generate API Key'}
            </button>
          </div>
        )}

        {/* Step 1: Show Key + Install */}
        {step === 1 && apiKey && (
          <div className="space-y-6">
            <div className="text-center">
              <h1 className="text-2xl font-bold text-white">Your API key</h1>
              <p className="text-[#8888a0] mt-2">
                Copy it now — you won't be able to see it again.
              </p>
            </div>

            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4">
              <div className="flex items-center justify-between gap-3">
                <code className="text-[#00FFB2] text-sm font-mono break-all flex-1">{apiKey}</code>
                <button
                  onClick={copyKey}
                  className="shrink-0 p-2 rounded-md hover:bg-[#2a2a3a] transition-colors"
                >
                  {copied ? <Check className="w-4 h-4 text-[#00FFB2]" /> : <Copy className="w-4 h-4 text-[#8888a0]" />}
                </button>
              </div>
            </div>

            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4 space-y-3">
              <p className="text-xs text-[#8888a0] uppercase tracking-wide">Install the SDK</p>
              <div className="bg-[#0a0a0f] rounded-md p-3 font-mono text-sm text-[#e8e8f0]">
                <span className="text-[#8888a0]">$</span> npm install @ato-sdk/js
              </div>
            </div>

            <button
              onClick={() => setStep(2)}
              className="w-full px-6 py-3 bg-[#00FFB2] text-black font-semibold rounded-lg hover:bg-[#00FFB2]/90 transition-colors flex items-center justify-center gap-2"
            >
              I've copied my key <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        )}

        {/* Step 2: Code Example */}
        {step === 2 && (
          <div className="space-y-6">
            <div className="text-center">
              <div className="w-12 h-12 rounded-xl bg-[#00FFB2]/10 flex items-center justify-center mx-auto mb-4">
                <Terminal className="w-6 h-6 text-[#00FFB2]" />
              </div>
              <h1 className="text-2xl font-bold text-white">Add to your code</h1>
              <p className="text-[#8888a0] mt-2">
                Wrap your LLM client — every call will be traced automatically.
              </p>
            </div>

            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg overflow-hidden">
              <div className="px-4 py-2 border-b border-[#2a2a3a] flex items-center gap-2">
                <div className="w-3 h-3 rounded-full bg-[#ef4444]" />
                <div className="w-3 h-3 rounded-full bg-[#eab308]" />
                <div className="w-3 h-3 rounded-full bg-[#22c55e]" />
                <span className="text-xs text-[#8888a0] ml-2">app.ts</span>
              </div>
              <pre className="p-4 text-sm font-mono text-[#e8e8f0] overflow-x-auto">{`import { init } from '@ato-sdk/js';
import { wrapAnthropic } from '@ato-sdk/js/anthropic';
import Anthropic from '@anthropic-ai/sdk';

// Initialize ATO
init({ apiKey: '${apiKey?.slice(0, 15) || 'ato_your_key'}...' });

// Wrap your client
const client = wrapAnthropic(new Anthropic());

// Use normally — traces are captured automatically
const msg = await client.messages.create({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Hello' }],
});`}</pre>
            </div>

            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4">
              <p className="text-sm text-[#8888a0]">
                Also works with <span className="text-white">OpenAI</span> (<code className="text-[#00FFB2]">wrapOpenAI</code>) and the <span className="text-white">Claude Agent SDK</span> (<code className="text-[#00FFB2]">wrapAgent</code>).
              </p>
              <a
                href="https://github.com/WillNigri/Agentic-Tool-Optimization/blob/main/docs/SDK.md"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1 text-xs text-[#00FFB2] hover:underline mt-2"
              >
                <ExternalLink className="w-3 h-3" /> Full SDK documentation
              </a>
            </div>

            <button
              onClick={() => {
                localStorage.setItem('ato-onboarding-complete', 'true');
                onComplete();
              }}
              className="w-full px-6 py-3 bg-[#00FFB2] text-black font-semibold rounded-lg hover:bg-[#00FFB2]/90 transition-colors"
            >
              Go to Dashboard
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
