import { useQuery } from "@tanstack/react-query";
import { Check, Copy, Loader2, RefreshCw, Server } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import {
  detectOllama,
  listOllamaModels,
  getOllamaConfig,
  type OllamaModel,
} from "@/lib/api";

function formatSize(bytes: number): string {
  if (bytes === 0) return "—";
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
}

export default function OllamaProvider() {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const { data: status, isLoading: statusLoading } = useQuery({
    queryKey: ["ollama-status"],
    queryFn: detectOllama,
    refetchInterval: 10_000,
    staleTime: 5_000,
  });

  const { data: models = [], isLoading: modelsLoading, refetch: refetchModels } = useQuery({
    queryKey: ["ollama-models"],
    queryFn: () => listOllamaModels(),
    enabled: !!status?.running,
    staleTime: 30_000,
  });

  const { data: config } = useQuery({
    queryKey: ["ollama-config"],
    queryFn: getOllamaConfig,
    staleTime: 60_000,
  });

  const running = status?.running ?? false;
  const endpoint = status?.endpoint ?? "http://localhost:11434";
  const openaiEndpoint = `${endpoint}/v1`;

  function handleCopyEndpoint() {
    navigator.clipboard.writeText(openaiEndpoint);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div className="rounded-xl border border-cs-border bg-cs-card">
      {/* Header */}
      <div className="flex items-start justify-between gap-3 border-b border-cs-border px-4 py-3">
        <div className="flex items-start gap-3 min-w-0">
          <div className={cn(
            "mt-0.5 rounded-md p-1.5",
            running ? "bg-green-500/10 text-green-400" : "bg-red-500/10 text-red-400"
          )}>
            <Server size={14} />
          </div>
          <div className="min-w-0">
            <h2 className="text-sm font-semibold flex items-center gap-2">
              {t("ollama.title", "Ollama")}
              <span className={cn(
                "inline-block h-2 w-2 rounded-full",
                running ? "bg-green-400" : "bg-red-400"
              )} />
              <span className="text-[10px] font-normal text-cs-muted">
                {statusLoading ? t("ollama.checking", "checking…") : running ? `v${status?.version ?? "?"}` : t("ollama.notRunning", "not running")}
              </span>
            </h2>
            <p className="mt-0.5 text-[11px] text-cs-muted">
              {t("ollama.subtitle", "Local model server — OpenAI-compatible endpoint for any runtime")}
            </p>
          </div>
        </div>
      </div>

      <div className="p-4 space-y-4">
        {/* Endpoint */}
        <div>
          <label className="mb-1.5 block text-[10px] font-medium text-cs-muted uppercase tracking-wide">
            {t("ollama.endpoint", "OpenAI-compatible endpoint")}
          </label>
          <div className="flex items-center gap-2">
            <code className="flex-1 truncate rounded-md border border-cs-border/60 bg-cs-bg px-3 py-1.5 font-mono text-xs">
              {openaiEndpoint}
            </code>
            <button
              onClick={handleCopyEndpoint}
              className="shrink-0 rounded-md border border-cs-border px-2.5 py-1.5 text-xs text-cs-muted transition-colors hover:bg-cs-border/50 hover:text-cs-text"
            >
              {copied ? <Check size={12} className="text-cs-accent" /> : <Copy size={12} />}
            </button>
          </div>
          <p className="mt-1 text-[10px] text-cs-muted">
            {t("ollama.endpointHint", "Paste this into any runtime's custom model endpoint to use Ollama models.")}
          </p>
        </div>

        {/* Models */}
        <div>
          <div className="mb-1.5 flex items-center justify-between">
            <label className="text-[10px] font-medium text-cs-muted uppercase tracking-wide">
              {t("ollama.models", "Available models")} ({models.length})
            </label>
            {running && (
              <button
                onClick={() => refetchModels()}
                className="text-cs-muted hover:text-cs-text"
              >
                <RefreshCw size={11} />
              </button>
            )}
          </div>
          {!running ? (
            <div className="rounded-md border border-dashed border-cs-border/60 bg-cs-bg/40 px-3 py-4 text-center text-xs text-cs-muted">
              {t("ollama.startToSee", "Start Ollama to see available models.")}
            </div>
          ) : modelsLoading ? (
            <div className="flex items-center gap-2 py-3 text-xs text-cs-muted">
              <Loader2 size={11} className="animate-spin" /> Loading models…
            </div>
          ) : models.length === 0 ? (
            <div className="rounded-md border border-dashed border-cs-border/60 bg-cs-bg/40 px-3 py-4 text-center text-xs text-cs-muted">
              {t("ollama.noModels", "No models installed.")} <code className="bg-cs-border/60 px-1 rounded">ollama pull llama3.2</code>
            </div>
          ) : (
            <ul className="max-h-56 space-y-1 overflow-y-auto">
              {models.map((m) => (
                <ModelRow key={m.digest || m.name} model={m} />
              ))}
            </ul>
          )}
        </div>

        {/* Config */}
        {config && (
          <div>
            <label className="mb-1.5 block text-[10px] font-medium text-cs-muted uppercase tracking-wide">
              {t("ollama.envConfig", "Environment config")}
            </label>
            <div className="grid grid-cols-2 gap-2">
              <ConfigField label="OLLAMA_HOST" value={config.host} notSetLabel={t("ollama.notSet", "not set")} />
              <ConfigField label="OLLAMA_MODELS" value={config.modelsDir} notSetLabel={t("ollama.notSet", "not set")} />
              <ConfigField label="OLLAMA_KEEP_ALIVE" value={config.keepAlive} notSetLabel={t("ollama.notSet", "not set")} />
              <ConfigField label="OLLAMA_NUM_PARALLEL" value={config.numParallel} notSetLabel={t("ollama.notSet", "not set")} />
              <ConfigField label="OLLAMA_FLASH_ATTENTION" value={config.flashAttention} notSetLabel={t("ollama.notSet", "not set")} />
              <ConfigField label="CUDA_VISIBLE_DEVICES" value={config.cudaVisibleDevices} notSetLabel={t("ollama.notSet", "not set")} />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function ModelRow({ model }: { model: OllamaModel }) {
  return (
    <li className="flex items-center gap-3 rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-1.5">
      <div className="min-w-0 flex-1">
        <span className="text-xs font-medium">{model.name}</span>
        {model.parameterSize && (
          <span className="ml-2 rounded bg-cs-accent/10 px-1.5 py-0.5 text-[10px] text-cs-accent">
            {model.parameterSize}
          </span>
        )}
        {model.quantization && (
          <span className="ml-1 rounded bg-cs-border/60 px-1.5 py-0.5 text-[10px] text-cs-muted">
            {model.quantization}
          </span>
        )}
      </div>
      <span className="shrink-0 font-mono text-[10px] text-cs-muted">
        {formatSize(model.size)}
      </span>
    </li>
  );
}

function ConfigField({ label, value, notSetLabel }: { label: string; value: string | null; notSetLabel?: string }) {
  return (
    <div className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-2 py-1.5">
      <div className="text-[9px] text-cs-muted font-mono">{label}</div>
      <div className={cn(
        "text-[11px] font-mono truncate",
        value ? "text-cs-text" : "text-cs-muted/50"
      )}>
        {value ?? (notSetLabel || "not set")}
      </div>
    </div>
  );
}
