import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Cpu,
  Save,
  Loader2,
  RefreshCw,
  Check,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listModelConfigs,
  saveModelConfig,
  type ModelConfig as ModelConfigType,
} from "@/lib/tauri-api";

const MODELS = {
  claude: [
    { id: "claude-sonnet-4-20250514", name: "Claude Sonnet 4", context: "200k" },
    { id: "claude-opus-4-20250514", name: "Claude Opus 4", context: "200k" },
    { id: "claude-3-5-sonnet-20241022", name: "Claude 3.5 Sonnet", context: "200k" },
    { id: "claude-3-5-haiku-20241022", name: "Claude 3.5 Haiku", context: "200k" },
  ],
  codex: [
    { id: "gpt-4-turbo", name: "GPT-4 Turbo", context: "128k" },
    { id: "gpt-4o", name: "GPT-4o", context: "128k" },
    { id: "gpt-4o-mini", name: "GPT-4o Mini", context: "128k" },
    { id: "o1-preview", name: "O1 Preview", context: "128k" },
  ],
  hermes: [
    { id: "llama-3.1-70b", name: "Llama 3.1 70B", context: "128k" },
    { id: "llama-3.1-8b", name: "Llama 3.1 8B", context: "128k" },
    { id: "mistral-large", name: "Mistral Large", context: "32k" },
    { id: "codestral", name: "Codestral", context: "32k" },
  ],
  openclaw: [
    { id: "claude-sonnet-4-20250514", name: "Claude Sonnet 4", context: "200k" },
    { id: "gpt-4-turbo", name: "GPT-4 Turbo", context: "128k" },
    { id: "custom", name: "Custom (via gateway)", context: "varies" },
  ],
};

const RUNTIMES = ["claude", "codex", "hermes", "openclaw"] as const;

const RUNTIME_COLORS: Record<string, string> = {
  claude: "border-orange-500/30 bg-orange-500/5",
  codex: "border-green-500/30 bg-green-500/5",
  hermes: "border-purple-500/30 bg-purple-500/5",
  openclaw: "border-cyan-500/30 bg-cyan-500/5",
};

const RUNTIME_TEXT_COLORS: Record<string, string> = {
  claude: "text-orange-400",
  codex: "text-green-400",
  hermes: "text-purple-400",
  openclaw: "text-cyan-400",
};

export default function ModelConfig() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  // Local state for edits
  const [editedConfigs, setEditedConfigs] = useState<Record<string, {
    modelId: string;
    maxTokens: number;
    temperature: number;
  }>>({});

  // Fetch existing configs
  const { data: configs = [], isLoading, refetch } = useQuery({
    queryKey: ["model-configs"],
    queryFn: listModelConfigs,
  });

  // Save mutation
  const saveMutation = useMutation({
    mutationFn: ({ runtime, modelId, maxTokens, temperature }: {
      runtime: string;
      modelId: string;
      maxTokens?: number;
      temperature?: number;
    }) => saveModelConfig(runtime, modelId, undefined, maxTokens, temperature),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["model-configs"] });
    },
  });

  const getConfigForRuntime = (runtime: string) => {
    return configs.find((c) => c.runtime === runtime && !c.projectId);
  };

  const getEditedConfig = (runtime: string) => {
    if (editedConfigs[runtime]) {
      return editedConfigs[runtime];
    }
    const existing = getConfigForRuntime(runtime);
    return {
      modelId: existing?.modelId || MODELS[runtime as keyof typeof MODELS][0].id,
      maxTokens: existing?.maxTokens || 8192,
      temperature: existing?.temperature || 0.7,
    };
  };

  const handleChange = (runtime: string, field: string, value: string | number) => {
    setEditedConfigs((prev) => ({
      ...prev,
      [runtime]: {
        ...getEditedConfig(runtime),
        [field]: value,
      },
    }));
  };

  const handleSave = (runtime: string) => {
    const config = getEditedConfig(runtime);
    saveMutation.mutate({
      runtime,
      modelId: config.modelId,
      maxTokens: config.maxTokens,
      temperature: config.temperature,
    });
  };

  const hasChanges = (runtime: string) => {
    const existing = getConfigForRuntime(runtime);
    const edited = editedConfigs[runtime];
    if (!edited) return false;

    return (
      edited.modelId !== (existing?.modelId || MODELS[runtime as keyof typeof MODELS][0].id) ||
      edited.maxTokens !== (existing?.maxTokens || 8192) ||
      edited.temperature !== (existing?.temperature || 0.7)
    );
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
            <Cpu className="text-cs-accent" size={24} />
            {t("models.title", "Model Configuration")}
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            {t("models.subtitle", "Configure default models and parameters for each runtime")}
          </p>
        </div>
        <button
          onClick={() => refetch()}
          className="p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors"
        >
          <RefreshCw size={16} />
        </button>
      </div>

      {/* Runtime cards */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {RUNTIMES.map((runtime) => {
          const config = getEditedConfig(runtime);
          const models = MODELS[runtime];
          const savedConfig = getConfigForRuntime(runtime);
          const changed = hasChanges(runtime);

          return (
            <div
              key={runtime}
              className={cn(
                "rounded-lg border p-4",
                RUNTIME_COLORS[runtime]
              )}
            >
              <div className="flex items-center justify-between mb-4">
                <h3 className={cn("font-semibold capitalize", RUNTIME_TEXT_COLORS[runtime])}>
                  {runtime}
                </h3>
                {savedConfig && (
                  <span className="text-xs text-cs-muted flex items-center gap-1">
                    <Check size={12} className="text-green-400" />
                    Configured
                  </span>
                )}
              </div>

              <div className="space-y-4">
                {/* Model selection */}
                <div>
                  <label className="block text-sm font-medium mb-1.5">Model</label>
                  <select
                    value={config.modelId}
                    onChange={(e) => handleChange(runtime, "modelId", e.target.value)}
                    className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
                  >
                    {models.map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.name} ({model.context})
                      </option>
                    ))}
                  </select>
                </div>

                {/* Max tokens */}
                <div>
                  <label className="block text-sm font-medium mb-1.5">
                    Max Tokens
                    <span className="text-cs-muted font-normal ml-2">{config.maxTokens}</span>
                  </label>
                  <input
                    type="range"
                    min="256"
                    max="32768"
                    step="256"
                    value={config.maxTokens}
                    onChange={(e) => handleChange(runtime, "maxTokens", parseInt(e.target.value))}
                    className="w-full"
                  />
                  <div className="flex justify-between text-xs text-cs-muted mt-1">
                    <span>256</span>
                    <span>32,768</span>
                  </div>
                </div>

                {/* Temperature */}
                <div>
                  <label className="block text-sm font-medium mb-1.5">
                    Temperature
                    <span className="text-cs-muted font-normal ml-2">{config.temperature}</span>
                  </label>
                  <input
                    type="range"
                    min="0"
                    max="2"
                    step="0.1"
                    value={config.temperature}
                    onChange={(e) => handleChange(runtime, "temperature", parseFloat(e.target.value))}
                    className="w-full"
                  />
                  <div className="flex justify-between text-xs text-cs-muted mt-1">
                    <span>0 (Deterministic)</span>
                    <span>2 (Creative)</span>
                  </div>
                </div>

                {/* Save button */}
                <button
                  onClick={() => handleSave(runtime)}
                  disabled={!changed || saveMutation.isPending}
                  className={cn(
                    "w-full flex items-center justify-center gap-2 px-4 py-2 rounded-md text-sm font-medium transition-colors",
                    changed
                      ? "bg-cs-accent text-black hover:bg-cs-accent/90"
                      : "bg-cs-border text-cs-muted cursor-not-allowed"
                  )}
                >
                  {saveMutation.isPending ? (
                    <Loader2 size={14} className="animate-spin" />
                  ) : (
                    <Save size={14} />
                  )}
                  {changed ? "Save Changes" : "No Changes"}
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* Info */}
      <div className="p-4 rounded-lg border border-cs-border bg-cs-card">
        <p className="text-sm text-cs-muted">
          These settings configure the default model and parameters for each runtime.
          Project-specific overrides can be set in the Agent Config section.
          Changes here affect all new conversations but not ongoing ones.
        </p>
      </div>
    </div>
  );
}
