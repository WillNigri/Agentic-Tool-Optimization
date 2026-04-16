import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Key,
  Plus,
  Eye,
  EyeOff,
  RotateCw,
  Trash2,
  Copy,
  Check,
  Power,
  PowerOff,
  Shield,
  Clock,
  Hash,
  ExternalLink,
} from "lucide-react";
import {
  listLlmApiKeys,
  saveLlmApiKey,
  getLlmApiKeyValue,
  rotateLlmApiKey,
  toggleLlmApiKey,
  deleteLlmApiKey,
  type LlmApiKey,
} from "@/lib/api";
import { cn } from "@/lib/utils";

const LLM_PROVIDERS = [
  { id: "anthropic", name: "Anthropic", prefix: "sk-ant-", color: "#D4A574", docsUrl: "https://console.anthropic.com/settings/keys" },
  { id: "openai", name: "OpenAI", prefix: "sk-", color: "#74AA9C", docsUrl: "https://platform.openai.com/api-keys" },
  { id: "google", name: "Google AI", prefix: "AI", color: "#4285F4", docsUrl: "https://aistudio.google.com/apikey" },
  { id: "mistral", name: "Mistral", prefix: "", color: "#FF7000", docsUrl: "https://console.mistral.ai/api-keys" },
  { id: "cohere", name: "Cohere", prefix: "", color: "#39594D", docsUrl: "https://dashboard.cohere.com/api-keys" },
  { id: "groq", name: "Groq", prefix: "gsk_", color: "#F55036", docsUrl: "https://console.groq.com/keys" },
  { id: "together", name: "Together AI", prefix: "", color: "#0066FF", docsUrl: "https://api.together.xyz/settings/api-keys" },
  { id: "fireworks", name: "Fireworks", prefix: "fw_", color: "#FF6B35", docsUrl: "https://fireworks.ai/account/api-keys" },
  { id: "custom", name: "Custom Provider", prefix: "", color: "#888", docsUrl: "" },
];

function getProvider(id: string) {
  return LLM_PROVIDERS.find((p) => p.id === id) || LLM_PROVIDERS[LLM_PROVIDERS.length - 1];
}

function formatTimeAgo(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const seconds = Math.floor((now.getTime() - date.getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  if (seconds < 604800) return `${Math.floor(seconds / 86400)}d ago`;
  return date.toLocaleDateString();
}

export default function LlmApiKeys() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showAddForm, setShowAddForm] = useState(false);
  const [revealedKeys, setRevealedKeys] = useState<Record<string, string>>({});
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [rotatingId, setRotatingId] = useState<string | null>(null);
  const [newRotateKey, setNewRotateKey] = useState("");

  // Form state
  const [formProvider, setFormProvider] = useState("anthropic");
  const [formName, setFormName] = useState("");
  const [formKey, setFormKey] = useState("");
  const [formRuntime, setFormRuntime] = useState("");

  const { data: keys = [], isLoading } = useQuery({
    queryKey: ["llm-api-keys"],
    queryFn: () => listLlmApiKeys(),
  });

  const saveMutation = useMutation({
    mutationFn: () => saveLlmApiKey(formProvider, formName || getProvider(formProvider).name, formKey, undefined, formRuntime || undefined),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["llm-api-keys"] });
      setShowAddForm(false);
      setFormProvider("anthropic");
      setFormName("");
      setFormKey("");
      setFormRuntime("");
    },
  });

  const toggleMutation = useMutation({
    mutationFn: ({ id, isActive }: { id: string; isActive: boolean }) => toggleLlmApiKey(id, isActive),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["llm-api-keys"] }),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteLlmApiKey(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["llm-api-keys"] }),
  });

  const rotateMutation = useMutation({
    mutationFn: ({ id, newKey }: { id: string; newKey: string }) => rotateLlmApiKey(id, newKey),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["llm-api-keys"] });
      setRotatingId(null);
      setNewRotateKey("");
    },
  });

  const revealKey = async (id: string) => {
    if (revealedKeys[id]) {
      setRevealedKeys((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      return;
    }
    try {
      const value = await getLlmApiKeyValue(id);
      setRevealedKeys((prev) => ({ ...prev, [id]: value }));
      setTimeout(() => {
        setRevealedKeys((prev) => {
          const next = { ...prev };
          delete next[id];
          return next;
        });
      }, 30000);
    } catch (err) {
      console.error("Failed to reveal key:", err);
    }
  };

  const copyKey = async (id: string) => {
    try {
      const value = revealedKeys[id] || (await getLlmApiKeyValue(id));
      await navigator.clipboard.writeText(value);
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 2000);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  };

  if (isLoading) {
    return (
      <div className="space-y-6 animate-pulse">
        <div className="h-8 bg-cs-border/30 rounded w-48" />
        {[1, 2, 3].map((i) => (
          <div key={i} className="card h-20" />
        ))}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Key className="w-5 h-5 text-cs-accent" />
            LLM API Keys
          </h2>
          <p className="text-cs-muted text-sm">
            Manage API keys for your LLM providers
          </p>
        </div>
        <button
          onClick={() => setShowAddForm(!showAddForm)}
          className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors"
        >
          <Plus className="w-3.5 h-3.5" />
          Add Key
        </button>
      </div>

      {/* Add Form */}
      {showAddForm && (
        <div className="card p-5 space-y-4 border-cs-accent/30">
          <h3 className="text-sm font-medium">Add New API Key</h3>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="text-xs text-cs-muted uppercase tracking-wide mb-1 block">Provider</label>
              <select
                value={formProvider}
                onChange={(e) => setFormProvider(e.target.value)}
                className="w-full px-3 py-2 bg-cs-border/30 border border-cs-border rounded-md text-sm"
              >
                {LLM_PROVIDERS.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </select>
            </div>
            <div>
              <label className="text-xs text-cs-muted uppercase tracking-wide mb-1 block">Display Name</label>
              <input
                type="text"
                placeholder={getProvider(formProvider).name}
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                className="w-full px-3 py-2 bg-cs-border/30 border border-cs-border rounded-md text-sm"
              />
            </div>
          </div>

          <div>
            <label className="text-xs text-cs-muted uppercase tracking-wide mb-1 block">API Key</label>
            <input
              type="password"
              placeholder={`${getProvider(formProvider).prefix}...`}
              value={formKey}
              onChange={(e) => setFormKey(e.target.value)}
              className="w-full px-3 py-2 bg-cs-border/30 border border-cs-border rounded-md text-sm font-mono"
            />
          </div>

          <div>
            <label className="text-xs text-cs-muted uppercase tracking-wide mb-1 block">Runtime (optional)</label>
            <select
              value={formRuntime}
              onChange={(e) => setFormRuntime(e.target.value)}
              className="w-full px-3 py-2 bg-cs-border/30 border border-cs-border rounded-md text-sm"
            >
              <option value="">All Runtimes</option>
              <option value="claude">Claude Code</option>
              <option value="codex">Codex</option>
              <option value="openclaw">OpenClaw</option>
              <option value="hermes">Hermes</option>
            </select>
          </div>

          {getProvider(formProvider).docsUrl && (
            <a
              href={getProvider(formProvider).docsUrl}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
            >
              <ExternalLink className="w-3 h-3" />
              Get your {getProvider(formProvider).name} API key
            </a>
          )}

          <div className="flex items-center gap-2 pt-2">
            <button
              onClick={() => saveMutation.mutate()}
              disabled={!formKey || saveMutation.isPending}
              className="px-4 py-2 text-sm rounded-md bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 disabled:opacity-50 transition-colors"
            >
              {saveMutation.isPending ? "Saving..." : "Save Key"}
            </button>
            <button
              onClick={() => setShowAddForm(false)}
              className="px-4 py-2 text-sm rounded-md bg-cs-border/50 hover:bg-cs-border transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Security Notice */}
      <div className="card p-3 flex items-start gap-3 border-yellow-500/20 bg-yellow-500/5">
        <Shield className="w-4 h-4 text-yellow-500 mt-0.5 shrink-0" />
        <p className="text-xs text-cs-muted">
          API keys are stored locally on your machine with base64 encoding. For production use, we recommend using OS keychain integration. Keys are never sent to any external server.
        </p>
      </div>

      {/* Keys List */}
      {keys.length === 0 ? (
        <div className="card text-center py-12">
          <Key className="w-8 h-8 text-cs-muted mx-auto mb-3" />
          <p className="text-cs-muted">No API keys configured</p>
          <p className="text-cs-muted text-xs mt-1">Add your first LLM provider key to get started</p>
        </div>
      ) : (
        <div className="space-y-2">
          {keys.map((key) => {
            const provider = getProvider(key.provider);
            const isRevealed = !!revealedKeys[key.id];
            const isRotating = rotatingId === key.id;

            return (
              <div
                key={key.id}
                className={cn(
                  "card px-4 py-3 hover:border-cs-accent/20 transition-colors",
                  !key.isActive && "opacity-50"
                )}
              >
                <div className="flex items-center gap-3">
                  <div
                    className="w-8 h-8 rounded-md flex items-center justify-center text-xs font-bold text-white shrink-0"
                    style={{ backgroundColor: provider.color }}
                  >
                    {provider.name[0]}
                  </div>

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium">{key.name}</span>
                      <span className="text-xs px-1.5 py-0.5 rounded bg-cs-border/50 text-cs-muted">
                        {provider.name}
                      </span>
                      {key.runtime && (
                        <span className="text-xs px-1.5 py-0.5 rounded bg-cs-accent/10 text-cs-accent">
                          {key.runtime}
                        </span>
                      )}
                      {!key.isActive && (
                        <span className="text-xs px-1.5 py-0.5 rounded bg-red-500/10 text-red-400">
                          disabled
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-3 mt-1">
                      <code className="text-xs text-cs-muted font-mono">
                        {isRevealed ? revealedKeys[key.id] : key.keyPreview}
                      </code>
                      <span className="text-xs text-cs-muted flex items-center gap-1">
                        <Hash className="w-3 h-3" />
                        {key.usageCount} uses
                      </span>
                      {key.lastUsed && (
                        <span className="text-xs text-cs-muted flex items-center gap-1">
                          <Clock className="w-3 h-3" />
                          {formatTimeAgo(key.lastUsed)}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-1">
                    <button
                      onClick={() => revealKey(key.id)}
                      className="p-1.5 rounded hover:bg-cs-border/50 transition-colors"
                      title={isRevealed ? "Hide" : "Reveal"}
                    >
                      {isRevealed ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                    </button>
                    <button
                      onClick={() => copyKey(key.id)}
                      className="p-1.5 rounded hover:bg-cs-border/50 transition-colors"
                      title="Copy"
                    >
                      {copiedId === key.id ? (
                        <Check className="w-3.5 h-3.5 text-cs-accent" />
                      ) : (
                        <Copy className="w-3.5 h-3.5" />
                      )}
                    </button>
                    <button
                      onClick={() => setRotatingId(isRotating ? null : key.id)}
                      className="p-1.5 rounded hover:bg-cs-border/50 transition-colors"
                      title="Rotate"
                    >
                      <RotateCw className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={() => toggleMutation.mutate({ id: key.id, isActive: !key.isActive })}
                      className="p-1.5 rounded hover:bg-cs-border/50 transition-colors"
                      title={key.isActive ? "Disable" : "Enable"}
                    >
                      {key.isActive ? (
                        <Power className="w-3.5 h-3.5 text-emerald-400" />
                      ) : (
                        <PowerOff className="w-3.5 h-3.5 text-red-400" />
                      )}
                    </button>
                    <button
                      onClick={() => {
                        if (confirm(`Delete API key "${key.name}"?`)) {
                          deleteMutation.mutate(key.id);
                        }
                      }}
                      className="p-1.5 rounded hover:bg-red-500/20 transition-colors text-red-400"
                      title="Delete"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>

                {isRotating && (
                  <div className="mt-3 pt-3 border-t border-cs-border/30 flex items-center gap-2">
                    <input
                      type="password"
                      placeholder="Enter new API key..."
                      value={newRotateKey}
                      onChange={(e) => setNewRotateKey(e.target.value)}
                      className="flex-1 px-3 py-1.5 bg-cs-border/30 border border-cs-border rounded-md text-sm font-mono"
                    />
                    <button
                      onClick={() => rotateMutation.mutate({ id: key.id, newKey: newRotateKey })}
                      disabled={!newRotateKey || rotateMutation.isPending}
                      className="px-3 py-1.5 text-sm rounded-md bg-cs-accent text-cs-bg font-medium disabled:opacity-50"
                    >
                      Rotate
                    </button>
                    <button
                      onClick={() => { setRotatingId(null); setNewRotateKey(""); }}
                      className="px-3 py-1.5 text-sm rounded-md bg-cs-border/50 hover:bg-cs-border"
                    >
                      Cancel
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
