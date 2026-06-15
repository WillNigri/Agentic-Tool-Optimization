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

        {/* Step 1: Show Key + What it's for */}
        {step === 1 && apiKey && (
          <div className="space-y-6">
            <div className="text-center">
              <h1 className="text-2xl font-bold text-white">Your API key</h1>
              <p className="text-[#8888a0] mt-2 text-sm">
                This key tells ATO who's sending data to your dashboard. Copy it now — for
                security, we can't show it to you again.
              </p>
            </div>

            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4">
              <div className="flex items-center justify-between gap-3">
                <code className="text-[#00FFB2] text-sm font-mono break-all flex-1">{apiKey}</code>
                <button
                  onClick={copyKey}
                  className="shrink-0 p-2 rounded-md hover:bg-[#2a2a3a] transition-colors"
                  aria-label="Copy key"
                >
                  {copied ? <Check className="w-4 h-4 text-[#00FFB2]" /> : <Copy className="w-4 h-4 text-[#8888a0]" />}
                </button>
              </div>
              {copied && (
                <p className="text-xs text-[#00FFB2] mt-2">Copied to clipboard ✓</p>
              )}
            </div>

            <div className="bg-[#16161e]/50 border border-[#2a2a3a] rounded-lg p-4 space-y-2">
              <p className="text-xs uppercase tracking-wide text-[#8888a0]">What's this for?</p>
              <p className="text-sm text-[#aaaab8] leading-relaxed">
                When you make AI calls from your code (with Claude, OpenAI, etc), the ATO SDK
                tags each call with this key so it shows up in <span className="text-white">your</span> dashboard — not someone else's.
                Think of it like a username for your AI usage.
              </p>
            </div>

            <button
              onClick={() => setStep(2)}
              disabled={!copied}
              className="w-full px-6 py-3 bg-[#00FFB2] text-black font-semibold rounded-lg hover:bg-[#00FFB2]/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
            >
              {copied ? <>I've copied my key <ChevronRight className="w-4 h-4" /></> : 'Copy the key first ↑'}
            </button>
          </div>
        )}

        {/* Step 2: Three-step setup with full explanation */}
        {step === 2 && (
          <div className="space-y-6">
            <div className="text-center">
              <div className="w-12 h-12 rounded-xl bg-[#00FFB2]/10 flex items-center justify-center mx-auto mb-4">
                <Terminal className="w-6 h-6 text-[#00FFB2]" />
              </div>
              <h1 className="text-2xl font-bold text-white">Use your key in 3 steps</h1>
              <p className="text-[#8888a0] mt-2 text-sm">
                For a Node.js or TypeScript project. New to this? See the
                {' '}<a href="https://github.com/WillNigri/Agentic-Tool-Optimization/blob/main/docs/SDK.md#non-technical-setup" target="_blank" rel="noreferrer" className="text-[#00FFB2] hover:underline">non-technical setup guide</a>.
              </p>
            </div>

            {/* Step 1: Install */}
            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-5 space-y-3">
              <div className="flex items-center gap-3">
                <div className="w-6 h-6 rounded-full bg-[#00FFB2]/15 text-[#00FFB2] text-xs font-bold flex items-center justify-center">1</div>
                <p className="text-sm font-semibold text-white">Install the ATO SDK</p>
              </div>
              <p className="text-xs text-[#8888a0] leading-relaxed pl-9">
                Open a terminal inside your project folder and run:
              </p>
              <div className="bg-[#0a0a0f] rounded-md p-3 font-mono text-sm text-[#e8e8f0] ml-9">
                <span className="text-[#8888a0]">$</span> npm install @ato-sdk/js
              </div>
            </div>

            {/* Step 2: Store the key safely */}
            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-5 space-y-3">
              <div className="flex items-center gap-3">
                <div className="w-6 h-6 rounded-full bg-[#00FFB2]/15 text-[#00FFB2] text-xs font-bold flex items-center justify-center">2</div>
                <p className="text-sm font-semibold text-white">Save the key as an environment variable</p>
              </div>
              <p className="text-xs text-[#8888a0] leading-relaxed pl-9">
                Create a file called <code className="text-[#00FFB2] bg-[#0a0a0f] px-1.5 py-0.5 rounded">.env</code> at the root of your project (same folder as <code className="text-[#00FFB2] bg-[#0a0a0f] px-1.5 py-0.5 rounded">package.json</code>) and paste this line into it. Don't commit <code className="text-[#00FFB2] bg-[#0a0a0f] px-1.5 py-0.5 rounded">.env</code> to git — add it to <code className="text-[#00FFB2] bg-[#0a0a0f] px-1.5 py-0.5 rounded">.gitignore</code>.
              </p>
              <div className="bg-[#0a0a0f] rounded-md p-3 font-mono text-xs text-[#e8e8f0] ml-9 overflow-x-auto">
                ATO_API_KEY={apiKey || 'ato_your_key_here'}
              </div>
              <p className="text-[11px] text-[#5a5a6e] leading-relaxed pl-9">
                Why not paste it directly in your code? If you push to GitHub, anyone can read it and run up your bill.
                The <code className="text-[#00FFB2]">.env</code> file stays on your machine.
              </p>
            </div>

            {/* Step 3: Wrap your client */}
            <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-5 space-y-3">
              <div className="flex items-center gap-3">
                <div className="w-6 h-6 rounded-full bg-[#00FFB2]/15 text-[#00FFB2] text-xs font-bold flex items-center justify-center">3</div>
                <p className="text-sm font-semibold text-white">Add ATO to your code</p>
              </div>
              <p className="text-xs text-[#8888a0] leading-relaxed pl-9">
                Wherever you create an Anthropic or OpenAI client, wrap it with ATO. Every call after this is auto-traced — no other code changes.
              </p>
              <div className="bg-[#0a0a0f] rounded-md overflow-hidden ml-9 border border-[#2a2a3a]">
                <div className="px-3 py-1.5 border-b border-[#2a2a3a] flex items-center gap-1.5">
                  <div className="w-2.5 h-2.5 rounded-full bg-[#ef4444]" />
                  <div className="w-2.5 h-2.5 rounded-full bg-[#eab308]" />
                  <div className="w-2.5 h-2.5 rounded-full bg-[#22c55e]" />
                  <span className="text-[11px] text-[#8888a0] ml-1.5">app.ts</span>
                </div>
                <pre className="p-3 text-xs font-mono text-[#e8e8f0] overflow-x-auto leading-relaxed">{`import { init } from '@ato-sdk/js';
import { wrapAnthropic } from '@ato-sdk/js/anthropic';
import Anthropic from '@anthropic-ai/sdk';

// 1. Tell ATO who you are (key from your .env file)
init({ apiKey: process.env.ATO_API_KEY });

// 2. Wrap your existing Anthropic client
const client = wrapAnthropic(new Anthropic());

// 3. Use it exactly like before — traces happen automatically
const msg = await client.messages.create({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Hello' }],
});`}</pre>
              </div>
            </div>

            {/* What to expect */}
            <div className="bg-[#00FFB2]/5 border border-[#00FFB2]/20 rounded-lg p-4 space-y-2">
              <p className="text-sm font-semibold text-[#00FFB2]">What happens next</p>
              <p className="text-xs text-[#aaaab8] leading-relaxed">
                Run your code and make any AI call. Within a few seconds, that call shows up here in your dashboard
                with full receipts: prompt, response, model, tokens used, cost, latency. Refresh this page to see it.
              </p>
            </div>

            {/* Also supported + docs */}
            <div className="bg-[#16161e]/50 border border-[#2a2a3a] rounded-lg p-4 space-y-2">
              <p className="text-xs text-[#8888a0]">
                Also works with <span className="text-white">OpenAI</span> (use <code className="text-[#00FFB2]">wrapOpenAI</code>),
                the <span className="text-white">Claude Agent SDK</span> (use <code className="text-[#00FFB2]">wrapAgent</code>),
                and any provider via the manual trace API.
              </p>
              <a
                href="https://github.com/WillNigri/Agentic-Tool-Optimization/blob/main/docs/SDK.md"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1 text-xs text-[#00FFB2] hover:underline"
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
