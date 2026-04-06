import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { FolderTree, Shield, Eye, Plus, RefreshCw, AlertTriangle, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAgentConfigStore, type RuntimeFilter } from "@/stores/useAgentConfigStore";
import {
  scanAgentConfigFiles,
  getAgentContextPreview,
  type AgentConfigRuntime,
  type AgentConfigFile,
} from "@/lib/tauri-api";
import ConfigFileExplorer from "./ConfigFileExplorer";
import ConfigFileEditor from "./ConfigFileEditor";
import PermissionMatrix from "./PermissionMatrix";
import ContextPreview from "./ContextPreview";
import CreateSkillModal from "./CreateSkillModal";

type Tab = "files" | "permissions" | "preview";

const RUNTIME_OPTIONS: { value: RuntimeFilter; label: string }[] = [
  { value: "all", label: "All Runtimes" },
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
  { value: "openclaw", label: "OpenClaw" },
  { value: "hermes", label: "Hermes" },
];

export default function AgentManager() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<Tab>("files");
  const [showCreateSkill, setShowCreateSkill] = useState(false);

  const {
    setConfigFiles,
    setContextPreview,
    activeRuntime,
    setActiveRuntime,
    selectedFilePath,
  } = useAgentConfigStore();

  // Fetch config files with error handling
  const {
    data: configFiles,
    isLoading,
    isError,
    error,
    refetch,
    isFetching,
  } = useQuery<AgentConfigFile[]>({
    queryKey: ["agent-config-files"],
    queryFn: async () => {
      console.log("[AgentManager] Fetching config files...");
      const result = await scanAgentConfigFiles();
      console.log("[AgentManager] Got config files:", result);
      return result;
    },
    retry: 1,
    staleTime: 30000,
  });

  // Update store when data changes
  useEffect(() => {
    if (configFiles) {
      setConfigFiles(configFiles);
    }
  }, [configFiles, setConfigFiles]);

  // Fetch context preview when runtime changes
  useEffect(() => {
    const runtime = activeRuntime !== "all" ? activeRuntime : "claude";
    getAgentContextPreview(runtime as AgentConfigRuntime)
      .then(setContextPreview)
      .catch((err) => {
        console.error("[AgentManager] Failed to get context preview:", err);
        setContextPreview(null);
      });
  }, [activeRuntime, setContextPreview]);

  const tabs: { id: Tab; label: string; icon: typeof FolderTree }[] = [
    { id: "files", label: t("agentManager.tabs.files", "Config Files"), icon: FolderTree },
    { id: "permissions", label: t("agentManager.tabs.permissions", "Permissions"), icon: Shield },
    { id: "preview", label: t("agentManager.tabs.preview", "Context Preview"), icon: Eye },
  ];

  // Loading state
  if (isLoading) {
    return (
      <div className="h-full flex flex-col items-center justify-center bg-cs-bg p-6">
        <Loader2 size={32} className="animate-spin text-cs-accent mb-4" />
        <p className="text-cs-muted">Loading agent configurations...</p>
      </div>
    );
  }

  // Error state
  if (isError) {
    return (
      <div className="h-full flex flex-col items-center justify-center bg-cs-bg p-6">
        <AlertTriangle size={48} className="mb-4 text-yellow-500" />
        <h2 className="text-lg font-semibold mb-2">Failed to load config files</h2>
        <p className="text-sm text-cs-muted mb-4 max-w-md text-center">
          {error instanceof Error ? error.message : "Unknown error occurred"}
        </p>
        <button
          onClick={() => refetch()}
          className="px-4 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-cs-bg">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-semibold">
            {t("agentManager.title", "Agent Configuration")}
          </h1>
          <p className="text-sm text-cs-muted mt-1">
            {t("agentManager.subtitle", "Manage config files for all your AI coding agents")}
          </p>
        </div>

        <div className="flex items-center gap-3">
          {/* Runtime filter */}
          <select
            value={activeRuntime}
            onChange={(e) => setActiveRuntime(e.target.value as RuntimeFilter)}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-1.5 text-sm focus:outline-none focus:border-cs-accent"
          >
            {RUNTIME_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>

          {/* Actions */}
          <button
            onClick={() => refetch()}
            disabled={isFetching}
            className="p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors disabled:opacity-50"
            title={t("common.refresh", "Refresh")}
          >
            <RefreshCw size={16} className={isFetching ? "animate-spin" : ""} />
          </button>

          <button
            onClick={() => setShowCreateSkill(true)}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Plus size={16} />
            {t("agentManager.newSkill", "New Skill")}
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 mb-4 border-b border-cs-border">
        {tabs.map((tab) => {
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                "flex items-center gap-2 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors",
                activeTab === tab.id
                  ? "border-cs-accent text-cs-accent"
                  : "border-transparent text-cs-muted hover:text-cs-text"
              )}
            >
              <Icon size={16} />
              {tab.label}
            </button>
          );
        })}
      </div>

      {/* Tab content */}
      <div className="flex-1 min-h-0">
        {activeTab === "files" && (
          <div className="h-full flex gap-4">
            {/* Left: File explorer */}
            <div className="w-80 shrink-0 border border-cs-border rounded-lg overflow-hidden">
              <ConfigFileExplorer isLoading={isFetching} />
            </div>

            {/* Right: Editor */}
            <div className="flex-1 border border-cs-border rounded-lg overflow-hidden">
              {selectedFilePath ? (
                <ConfigFileEditor />
              ) : (
                <div className="h-full flex items-center justify-center text-cs-muted bg-cs-card">
                  <div className="text-center">
                    <FolderTree size={48} className="mx-auto mb-3 opacity-50" />
                    <p>{t("agentManager.selectFile", "Select a config file to edit")}</p>
                    <p className="text-xs mt-2 opacity-70">
                      {configFiles?.length || 0} files discovered
                    </p>
                  </div>
                </div>
              )}
            </div>
          </div>
        )}

        {activeTab === "permissions" && <PermissionMatrix />}

        {activeTab === "preview" && <ContextPreview />}
      </div>

      {/* Create skill modal */}
      {showCreateSkill && (
        <CreateSkillModal
          onClose={() => setShowCreateSkill(false)}
          onCreated={() => {
            setShowCreateSkill(false);
            refetch();
          }}
        />
      )}
    </div>
  );
}
