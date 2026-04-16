import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Key,
  Plus,
  Eye,
  EyeOff,
  Trash2,
  Edit2,
  Check,
  X,
  Loader2,
  AlertTriangle,
  Shield,
  RefreshCw,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listSecrets,
  saveSecret,
  getSecretValue,
  updateSecret,
  deleteSecret,
  type Secret,
} from "@/lib/api";

const KEY_TYPES = [
  { value: "api_key", label: "API Key", icon: Key },
  { value: "token", label: "Token", icon: Shield },
  { value: "ssh_key", label: "SSH Key Path", icon: Key },
];

const RUNTIMES = [
  { value: "", label: "Global (All Runtimes)" },
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
  { value: "hermes", label: "Hermes" },
  { value: "openclaw", label: "OpenClaw" },
];

export default function SecretsManager() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [revealedIds, setRevealedIds] = useState<Set<string>>(new Set());
  const [revealedValues, setRevealedValues] = useState<Record<string, string>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");

  // Form state
  const [newName, setNewName] = useState("");
  const [newKeyType, setNewKeyType] = useState("api_key");
  const [newValue, setNewValue] = useState("");
  const [newRuntime, setNewRuntime] = useState("");

  // Fetch secrets
  const { data: secrets = [], isLoading, refetch } = useQuery({
    queryKey: ["secrets"],
    queryFn: listSecrets,
  });

  // Save mutation
  const saveMutation = useMutation({
    mutationFn: () => saveSecret(newName, newKeyType, newValue, newRuntime || undefined),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["secrets"] });
      setShowAdd(false);
      setNewName("");
      setNewValue("");
      setNewKeyType("api_key");
      setNewRuntime("");
    },
  });

  // Update mutation
  const updateMutation = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => updateSecret(id, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["secrets"] });
      setEditingId(null);
    },
  });

  // Delete mutation
  const deleteMutation = useMutation({
    mutationFn: deleteSecret,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["secrets"] });
    },
  });

  const handleReveal = async (secretId: string) => {
    if (revealedIds.has(secretId)) {
      // Hide
      const newRevealed = new Set(revealedIds);
      newRevealed.delete(secretId);
      setRevealedIds(newRevealed);
      const newValues = { ...revealedValues };
      delete newValues[secretId];
      setRevealedValues(newValues);
    } else {
      // Reveal
      try {
        const value = await getSecretValue(secretId);
        setRevealedIds(new Set([...revealedIds, secretId]));
        setRevealedValues({ ...revealedValues, [secretId]: value });
        // Auto-hide after 30 seconds
        setTimeout(() => {
          setRevealedIds((prev) => {
            const next = new Set(prev);
            next.delete(secretId);
            return next;
          });
          setRevealedValues((prev) => {
            const next = { ...prev };
            delete next[secretId];
            return next;
          });
        }, 30000);
      } catch (err) {
        console.error("Failed to reveal secret:", err);
      }
    }
  };

  const startEdit = (secret: Secret) => {
    setEditingId(secret.id);
    setEditName(secret.name);
  };

  const saveEdit = () => {
    if (editingId && editName.trim()) {
      updateMutation.mutate({ id: editingId, name: editName.trim() });
    }
  };

  const getKeyTypeLabel = (type: string) => {
    return KEY_TYPES.find((k) => k.value === type)?.label || type;
  };

  const getRuntimeLabel = (runtime?: string) => {
    if (!runtime) return "Global";
    return RUNTIMES.find((r) => r.value === runtime)?.label || runtime;
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="animate-spin text-cs-accent" size={32} />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Shield className="text-cs-accent" size={24} />
            {t("secrets.title", "Secrets Manager")}
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            {t("secrets.subtitle", "Securely store API keys and tokens in your system keychain")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => refetch()}
            className="p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors"
          >
            <RefreshCw size={16} />
          </button>
          <button
            onClick={() => setShowAdd(true)}
            className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Plus size={16} />
            Add Secret
          </button>
        </div>
      </div>

      {/* Security notice */}
      <div className="flex items-start gap-3 p-4 rounded-lg border border-cs-accent/30 bg-cs-accent/5">
        <Shield size={20} className="text-cs-accent shrink-0 mt-0.5" />
        <div>
          <p className="text-sm font-medium text-cs-accent">Secure Storage</p>
          <p className="text-xs text-cs-muted mt-1">
            Secrets are stored in your operating system's secure keychain (macOS Keychain, Windows Credential Manager, or Linux Secret Service).
            ATO only stores metadata - never the actual secret values in its database.
          </p>
        </div>
      </div>

      {/* Add form */}
      {showAdd && (
        <div className="border border-cs-border rounded-lg p-4 bg-cs-card">
          <h3 className="font-medium mb-4">Add New Secret</h3>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium mb-1.5">Name</label>
              <input
                type="text"
                placeholder="e.g., ANTHROPIC_API_KEY"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              />
            </div>
            <div>
              <label className="block text-sm font-medium mb-1.5">Type</label>
              <select
                value={newKeyType}
                onChange={(e) => setNewKeyType(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              >
                {KEY_TYPES.map((type) => (
                  <option key={type.value} value={type.value}>
                    {type.label}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium mb-1.5">Runtime</label>
              <select
                value={newRuntime}
                onChange={(e) => setNewRuntime(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              >
                {RUNTIMES.map((rt) => (
                  <option key={rt.value} value={rt.value}>
                    {rt.label}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium mb-1.5">Value</label>
              <input
                type="password"
                placeholder="Enter secret value"
                value={newValue}
                onChange={(e) => setNewValue(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm font-mono focus:outline-none focus:border-cs-accent"
              />
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-4">
            <button
              onClick={() => setShowAdd(false)}
              className="px-4 py-2 rounded-md text-sm hover:bg-cs-border transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={() => saveMutation.mutate()}
              disabled={!newName.trim() || !newValue.trim() || saveMutation.isPending}
              className="flex items-center gap-2 px-4 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
            >
              {saveMutation.isPending && <Loader2 size={14} className="animate-spin" />}
              Save to Keychain
            </button>
          </div>
        </div>
      )}

      {/* Secrets list */}
      {secrets.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <Key size={48} className="mx-auto mb-4 opacity-50" />
          <p>No secrets stored yet</p>
          <p className="text-sm mt-1">Add your first API key or token</p>
        </div>
      ) : (
        <div className="space-y-2">
          {secrets.map((secret) => (
            <div
              key={secret.id}
              className="flex items-center justify-between p-4 rounded-lg border border-cs-border bg-cs-card"
            >
              <div className="flex items-center gap-4 flex-1 min-w-0">
                <div className="w-10 h-10 rounded-lg bg-cs-accent/10 flex items-center justify-center shrink-0">
                  <Key size={20} className="text-cs-accent" />
                </div>
                <div className="min-w-0 flex-1">
                  {editingId === secret.id ? (
                    <div className="flex items-center gap-2">
                      <input
                        type="text"
                        value={editName}
                        onChange={(e) => setEditName(e.target.value)}
                        className="px-2 py-1 rounded border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
                        autoFocus
                      />
                      <button onClick={saveEdit} className="p-1 text-green-400 hover:bg-cs-border rounded">
                        <Check size={14} />
                      </button>
                      <button onClick={() => setEditingId(null)} className="p-1 text-red-400 hover:bg-cs-border rounded">
                        <X size={14} />
                      </button>
                    </div>
                  ) : (
                    <p className="font-medium truncate">{secret.name}</p>
                  )}
                  <div className="flex items-center gap-2 mt-1">
                    <span className="text-xs px-2 py-0.5 rounded bg-cs-border text-cs-muted">
                      {getKeyTypeLabel(secret.keyType)}
                    </span>
                    <span className="text-xs text-cs-muted">
                      {getRuntimeLabel(secret.runtime)}
                    </span>
                  </div>
                </div>
              </div>

              {/* Value display */}
              <div className="flex items-center gap-2 mx-4">
                {revealedIds.has(secret.id) ? (
                  <code className="text-xs bg-cs-bg px-2 py-1 rounded font-mono max-w-[200px] truncate">
                    {revealedValues[secret.id]}
                  </code>
                ) : (
                  <code className="text-xs text-cs-muted">••••••••••••</code>
                )}
              </div>

              {/* Actions */}
              <div className="flex items-center gap-1">
                <button
                  onClick={() => handleReveal(secret.id)}
                  className="p-2 rounded hover:bg-cs-border transition-colors"
                  title={revealedIds.has(secret.id) ? "Hide" : "Reveal"}
                >
                  {revealedIds.has(secret.id) ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
                <button
                  onClick={() => startEdit(secret)}
                  className="p-2 rounded hover:bg-cs-border transition-colors"
                  title="Edit"
                >
                  <Edit2 size={16} />
                </button>
                <button
                  onClick={() => deleteMutation.mutate(secret.id)}
                  className="p-2 rounded hover:bg-cs-border text-red-400 transition-colors"
                  title="Delete"
                >
                  <Trash2 size={16} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Error display */}
      {saveMutation.isError && (
        <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
          <AlertTriangle size={16} />
          {saveMutation.error instanceof Error ? saveMutation.error.message : "Failed to save secret"}
        </div>
      )}
    </div>
  );
}
