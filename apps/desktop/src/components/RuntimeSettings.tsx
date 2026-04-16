import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import {
  Terminal, Globe, Cpu, Bot, ChevronDown, Check, X, Loader2,
  RefreshCw, Save, Wifi, WifiOff, Settings2, FileText, ExternalLink,
  FileCheck, FileX,
} from "lucide-react";
import { detectAgentRuntimes, queryAllAgentStatuses } from "@/lib/api";
import * as tauriApi from "@/lib/api";
import { getConfigFiles } from "@/lib/api";
import FileViewer from "./FileViewer";

// ---- Runtime metadata ----

interface RuntimeField {
  key: string;
  label: string;
  placeholder: string;
  type: string;
}

interface RuntimeMeta {
  id: "claude" | "openclaw" | "codex" | "hermes";
  name: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  color: string;
  description: string;
  fields: RuntimeField[];
}

const RUNTIMES: RuntimeMeta[] = [
  {
    id: "claude",
    name: "Claude Code",
    icon: Terminal,
    color: "#00FFB2",
    description: "Anthropic's CLI for Claude",
    fields: [
      { key: "path", label: "CLI Path Override", placeholder: "/usr/local/bin/claude", type: "text" },
    ],
  },
  {
    id: "openclaw",
    name: "OpenClaw",
    icon: Globe,
    color: "#f97316",
    description: "Remote AI agent gateway via SSH/WebSocket",
    fields: [
      { key: "sshHost", label: "SSH Host", placeholder: "server.example.com", type: "text" },
      { key: "sshPort", label: "SSH Port", placeholder: "22", type: "number" },
      { key: "sshUser", label: "SSH User", placeholder: "root", type: "text" },
      { key: "sshKeyPath", label: "SSH Key Path", placeholder: "~/.ssh/id_rsa", type: "text" },
      { key: "wsUrl", label: "Gateway WebSocket URL", placeholder: "ws://localhost:18789", type: "text" },
      { key: "token", label: "Gateway Token", placeholder: "Bearer token for API access", type: "password" },
    ],
  },
  {
    id: "codex",
    name: "Codex",
    icon: Cpu,
    color: "#3b82f6",
    description: "OpenAI's coding agent",
    fields: [
      { key: "path", label: "CLI Path Override", placeholder: "/usr/local/bin/codex", type: "text" },
      { key: "apiKeyPath", label: "API Key Path", placeholder: "~/.codex/api-key", type: "text" },
    ],
  },
  {
    id: "hermes",
    name: "Hermes",
    icon: Bot,
    color: "#a78bfa",
    description: "Local AI agent with persistent memory",
    fields: [
      { key: "path", label: "CLI Path Override", placeholder: "/usr/local/bin/hermes", type: "text" },
      { key: "endpoint", label: "Endpoint URL", placeholder: "http://localhost:3000", type: "text" },
    ],
  },
];

// ---- Status types ----

type ConnectionStatus = "connected" | "disconnected" | "testing" | "unknown";

interface RuntimeState {
  config: Record<string, string>;
  status: ConnectionStatus;
  version: string | null;
  testError: string | null;
  saveSuccess: boolean;
  dirty: boolean;
}

// ---- Component ----

export default function RuntimeSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [expanded, setExpanded] = useState<string | null>(null);
  const [runtimeStates, setRuntimeStates] = useState<Record<string, RuntimeState>>(() => {
    const initial: Record<string, RuntimeState> = {};
    for (const rt of RUNTIMES) {
      initial[rt.id] = {
        config: {},
        status: "unknown",
        version: null,
        testError: null,
        saveSuccess: false,
        dirty: false,
      };
    }
    return initial;
  });

  const [configFilesOpen, setConfigFilesOpen] = useState(false);
  const [viewingFile, setViewingFile] = useState<string | null>(null);

  // ---- Queries ----

  const { data: detectedRuntimes = [] } = useQuery({
    queryKey: ["detected-runtimes"],
    queryFn: detectAgentRuntimes,
    staleTime: 60_000,
  });

  const { data: agentStatuses = [] } = useQuery({
    queryKey: ["agent-statuses"],
    queryFn: queryAllAgentStatuses,
    staleTime: 30_000,
  });

  const { data: configFiles = [], isLoading: configFilesLoading } = useQuery({
    queryKey: ["config-files"],
    queryFn: getConfigFiles,
  });

  // ---- Load saved configs on mount ----

  useEffect(() => {
    for (const rt of RUNTIMES) {
      tauriApi.loadRuntimeConfig(rt.id).then((raw) => {
        if (raw) {
          try {
            const parsed = JSON.parse(raw);
            // If config has SSH host set, mark as potentially connected
            const hasConfig = parsed.sshHost || parsed.path || parsed.endpoint;
            updateState(rt.id, { config: parsed, ...(hasConfig ? { status: "connected" as const } : {}) });
          } catch {
            // ignore parse errors
          }
        }
      }).catch(() => {
        // Tauri not available — try localStorage fallback
        const stored = localStorage.getItem(`ato-runtime-config-${rt.id}`);
        if (stored) {
          try {
            updateState(rt.id, { config: JSON.parse(stored) });
          } catch {
            // ignore
          }
        }
      });
    }
  }, []);

  // ---- Sync detected runtime statuses ----

  useEffect(() => {
    for (const status of agentStatuses) {
      const rtId = status.runtime as string;
      if (runtimeStates[rtId]) {
        // Don't override a manually tested "connected" status
        const current = runtimeStates[rtId].status;
        if (current === "connected") continue;
        updateState(rtId, {
          status: status.healthy ? "connected" : status.available ? "disconnected" : "disconnected",
          version: status.version || runtimeStates[rtId].version,
        });
      }
    }
  }, [agentStatuses]);

  // ---- Helpers ----

  const updateState = useCallback((id: string, partial: Partial<RuntimeState>) => {
    setRuntimeStates((prev) => ({
      ...prev,
      [id]: { ...prev[id], ...partial },
    }));
  }, []);

  function setFieldValue(runtimeId: string, key: string, value: string) {
    setRuntimeStates((prev) => ({
      ...prev,
      [runtimeId]: {
        ...prev[runtimeId],
        config: { ...prev[runtimeId].config, [key]: value },
        dirty: true,
        saveSuccess: false,
      },
    }));
  }

  // ---- Save ----

  const saveMutation = useMutation({
    mutationFn: async ({ runtimeId, config }: { runtimeId: string; config: Record<string, string> }) => {
      const configStr = JSON.stringify(config);
      try {
        await tauriApi.saveRuntimeConfig(runtimeId, configStr);
      } catch {
        // Fallback to localStorage
        localStorage.setItem(`ato-runtime-config-${runtimeId}`, configStr);
      }
    },
    onSuccess: (_, { runtimeId }) => {
      updateState(runtimeId, { saveSuccess: true, dirty: false });
      setTimeout(() => updateState(runtimeId, { saveSuccess: false }), 2500);
    },
  });

  // ---- Test Connection ----

  const testMutation = useMutation({
    mutationFn: async ({ runtimeId, config }: { runtimeId: string; config: Record<string, string> }) => {
      updateState(runtimeId, { status: "testing", testError: null });
      try {
        const result = await tauriApi.testRuntimeConnection(runtimeId, JSON.stringify(config));
        return { runtimeId, result };
      } catch (err) {
        console.error("[ATO] testRuntimeConnection failed:", runtimeId, err);
        // For runtimes with SSH/remote config, show the actual error
        if (runtimeId === "openclaw" || runtimeId === "hermes") {
          return {
            runtimeId,
            result: {
              connected: false,
              error: err instanceof Error ? err.message : String(err),
            },
          };
        }
        // Fallback for local runtimes: use detectAgentRuntimes
        const detected = await detectAgentRuntimes();
        const match = detected.find((d) => d.runtime === runtimeId);
        return {
          runtimeId,
          result: {
            connected: match?.available ?? false,
            version: match?.version,
            error: match?.available ? undefined : "Runtime not detected on this system",
          },
        };
      }
    },
    onSuccess: ({ runtimeId, result }) => {
      // If we got a result at all, the connection worked
      // OpenClaw returns gateway status object, others return {connected, version}
      const r = result as Record<string, unknown> | null;
      const connected = r === null ? false
        : "connected" in r ? !!r.connected
        : true; // Any response means connection succeeded
      const version = r && "version" in r ? String(r.version) : runtimeStates[runtimeId].version;
      const error = r && "error" in r ? String(r.error) : null;
      updateState(runtimeId, {
        status: connected ? "connected" : "disconnected",
        version,
        testError: error,
      });
    },
    onError: (err, { runtimeId }) => {
      updateState(runtimeId, {
        status: "disconnected",
        testError: err instanceof Error ? err.message : "Connection test failed",
      });
    },
  });

  // ---- Refresh all statuses ----

  function refreshAll() {
    queryClient.invalidateQueries({ queryKey: ["detected-runtimes"] });
    queryClient.invalidateQueries({ queryKey: ["agent-statuses"] });
  }

  // ---- Status indicator helpers ----

  function statusDotClass(status: ConnectionStatus): string {
    switch (status) {
      case "connected": return "bg-cs-success";
      case "disconnected": return "bg-red-500";
      case "testing": return "bg-yellow-400 animate-pulse";
      case "unknown": return "bg-cs-muted/50";
    }
  }

  function statusLabel(status: ConnectionStatus): string {
    switch (status) {
      case "connected": return "Connected";
      case "disconnected": return "Disconnected";
      case "testing": return "Testing...";
      case "unknown": return "Unknown";
    }
  }

  // ---- Render ----

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold mb-1">
            <Settings2 size={20} className="inline-block mr-2 -mt-0.5 text-cs-accent" />
            Runtime Settings
          </h2>
          <p className="text-cs-muted text-sm">
            Configure and manage connections to AI agent runtimes
          </p>
        </div>
        <button
          onClick={refreshAll}
          className="p-2 rounded-lg hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
          title="Refresh all statuses"
        >
          <RefreshCw size={16} />
        </button>
      </div>

      {/* Runtime cards */}
      <div className="space-y-3">
        {RUNTIMES.map((rt) => {
          const state = runtimeStates[rt.id];
          const isExpanded = expanded === rt.id;
          const Icon = rt.icon;
          const detected = detectedRuntimes.find((d) => d.runtime === rt.id);

          return (
            <div
              key={rt.id}
              className={cn(
                "rounded-xl border transition-all duration-200",
                isExpanded
                  ? "bg-cs-card border-cs-border shadow-lg"
                  : "bg-cs-card/60 border-cs-border/60 hover:border-cs-border"
              )}
            >
              {/* Card header */}
              <button
                onClick={() => setExpanded(isExpanded ? null : rt.id)}
                className="w-full flex items-center gap-3 p-4 text-left"
              >
                <div
                  className="w-9 h-9 rounded-lg flex items-center justify-center shrink-0"
                  style={{ backgroundColor: `${rt.color}15` }}
                >
                  <Icon size={18} style={{ color: rt.color }} />
                </div>

                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-cs-text">{rt.name}</span>
                    <span
                      className={cn("w-2 h-2 rounded-full shrink-0", statusDotClass(state.status))}
                    />
                    {state.version && (
                      <span className="text-[10px] font-mono text-cs-muted bg-cs-border/50 px-1.5 py-0.5 rounded">
                        v{state.version}
                      </span>
                    )}
                    {detected?.available && (
                      <span className="text-[10px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded bg-cs-success/15 text-cs-success">
                        Detected
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-cs-muted mt-0.5">{rt.description}</p>
                </div>

                <div className="flex items-center gap-2 shrink-0">
                  <span className="text-[10px] text-cs-muted hidden sm:inline">
                    {statusLabel(state.status)}
                  </span>
                  <ChevronDown
                    size={16}
                    className={cn(
                      "text-cs-muted transition-transform duration-200",
                      isExpanded && "rotate-180"
                    )}
                  />
                </div>
              </button>

              {/* Expanded content */}
              {isExpanded && (
                <div className="px-4 pb-4 border-t border-cs-border/50 pt-4 space-y-4">
                  {/* Config fields */}
                  <div className="space-y-3">
                    {rt.fields.map((field) => (
                      <div key={field.key}>
                        <label className="block text-xs font-medium text-cs-muted mb-1.5">
                          {field.label}
                        </label>
                        <input
                          type={field.type}
                          placeholder={field.placeholder}
                          value={state.config[field.key] || ""}
                          onChange={(e) => setFieldValue(rt.id, field.key, e.target.value)}
                          className={cn(
                            "w-full px-3 py-2 rounded-lg text-sm font-mono",
                            "bg-cs-bg border border-cs-border",
                            "text-cs-text placeholder:text-cs-muted/40",
                            "focus:outline-none focus:border-cs-accent/50 focus:ring-1 focus:ring-cs-accent/20",
                            "transition-colors"
                          )}
                        />
                      </div>
                    ))}
                  </div>

                  {/* Test error display */}
                  {state.testError && (
                    <div className="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-red-500/10 border border-red-500/20">
                      <X size={14} className="text-red-400 shrink-0 mt-0.5" />
                      <p className="text-xs text-red-300">{state.testError}</p>
                    </div>
                  )}

                  {/* Save success display */}
                  {state.saveSuccess && (
                    <div className="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-cs-success/10 border border-cs-success/20">
                      <Check size={14} className="text-cs-success shrink-0" />
                      <p className="text-xs text-cs-success">Configuration saved successfully</p>
                    </div>
                  )}

                  {/* Action buttons */}
                  <div className="flex items-center gap-3 pt-1">
                    <button
                      onClick={() => testMutation.mutate({ runtimeId: rt.id, config: state.config })}
                      disabled={state.status === "testing"}
                      className={cn(
                        "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors",
                        "border border-cs-border hover:border-cs-accent/30 hover:bg-cs-accent/5",
                        "text-cs-text disabled:opacity-50 disabled:cursor-not-allowed"
                      )}
                    >
                      {state.status === "testing" ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : state.status === "connected" ? (
                        <Wifi size={14} className="text-cs-success" />
                      ) : (
                        <WifiOff size={14} className="text-cs-muted" />
                      )}
                      Test Connection
                    </button>

                    <button
                      onClick={() => saveMutation.mutate({ runtimeId: rt.id, config: state.config })}
                      disabled={saveMutation.isPending || !state.dirty}
                      className={cn(
                        "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors",
                        state.dirty
                          ? "bg-cs-accent text-cs-bg hover:bg-cs-accent/90"
                          : "bg-cs-border/50 text-cs-muted cursor-not-allowed"
                      )}
                    >
                      {saveMutation.isPending ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : state.saveSuccess ? (
                        <Check size={14} />
                      ) : (
                        <Save size={14} />
                      )}
                      Save
                    </button>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Config Files section */}
      <div className="rounded-xl border border-cs-border/60 bg-cs-card/60">
        <button
          onClick={() => setConfigFilesOpen(!configFilesOpen)}
          className="w-full flex items-center gap-3 p-4 text-left"
        >
          <FileText size={18} className="text-cs-muted shrink-0" />
          <div className="flex-1 min-w-0">
            <span className="text-sm font-semibold text-cs-text">Config Files</span>
            <p className="text-xs text-cs-muted mt-0.5">
              View raw configuration files on disk
            </p>
          </div>
          <span className="text-[10px] text-cs-muted font-mono mr-2">
            {configFiles.filter((f) => f.exists).length}/{configFiles.length} found
          </span>
          <ChevronDown
            size={16}
            className={cn(
              "text-cs-muted transition-transform duration-200",
              configFilesOpen && "rotate-180"
            )}
          />
        </button>

        {configFilesOpen && (
          <div className="px-4 pb-4 border-t border-cs-border/50 pt-3 space-y-2">
            {configFilesLoading ? (
              <div className="space-y-2 animate-pulse">
                {[1, 2, 3].map((i) => (
                  <div key={i} className="h-12 bg-cs-border/30 rounded-lg" />
                ))}
              </div>
            ) : configFiles.length === 0 ? (
              <p className="text-xs text-cs-muted text-center py-6">No configuration files found</p>
            ) : (
              configFiles.map((config) => (
                <div
                  key={config.path}
                  onClick={() => config.exists && setViewingFile(config.path)}
                  className={cn(
                    "flex items-center gap-3 px-3 py-2.5 rounded-lg transition-colors",
                    "bg-cs-bg/50 border border-cs-border/30",
                    config.exists && "cursor-pointer hover:border-cs-accent/30"
                  )}
                >
                  {config.exists ? (
                    <FileCheck size={16} className="text-cs-success shrink-0" />
                  ) : (
                    <FileX size={16} className="text-cs-muted/50 shrink-0" />
                  )}
                  <div className="min-w-0 flex-1">
                    <p className="text-xs font-mono truncate text-cs-text">{config.path}</p>
                    <p className="text-[10px] text-cs-muted mt-0.5">{config.scope}</p>
                  </div>
                  {config.exists && (
                    <ExternalLink size={12} className="text-cs-muted/40 shrink-0" />
                  )}
                </div>
              ))
            )}
          </div>
        )}
      </div>

      {/* File viewer slide-over */}
      {viewingFile && (
        <FileViewer filePath={viewingFile} onClose={() => setViewingFile(null)} />
      )}
    </div>
  );
}
