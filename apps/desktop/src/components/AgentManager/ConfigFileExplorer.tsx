import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronRight,
  ChevronDown,
  FileJson,
  FileText,
  Settings,
  Sparkles,
  Globe,
  FolderOpen,
  Server,
  Ghost,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useAgentConfigStore } from "@/stores/useAgentConfigStore";
import type { AgentConfigFile } from "@/lib/tauri-api";

interface Props {
  isLoading: boolean;
}

const FILE_TYPE_ICONS: Record<string, typeof FileJson> = {
  skill: Sparkles,
  settings: Settings,
  "project-config": FileText,
  mcp: Server,
  soul: Ghost,
};

const RUNTIME_COLORS: Record<string, string> = {
  claude: "text-orange-400",
  codex: "text-green-400",
  openclaw: "text-purple-400",
  hermes: "text-blue-400",
  shared: "text-cs-muted",
};

export default function ConfigFileExplorer({ isLoading }: Props) {
  const { t } = useTranslation();
  const [expandedSections, setExpandedSections] = useState<Record<string, boolean>>({
    global: true,
    project: true,
  });

  const {
    selectedFilePath,
    selectFile,
    getFilesByScope,
    activeRuntime,
  } = useAgentConfigStore();

  const globalFiles = getFilesByScope("global");
  const projectFiles = getFilesByScope("project");

  const toggleSection = (section: string) => {
    setExpandedSections((prev) => ({
      ...prev,
      [section]: !prev[section],
    }));
  };

  const groupFilesByRuntime = (files: AgentConfigFile[]) => {
    const grouped: Record<string, AgentConfigFile[]> = {};
    for (const file of files) {
      const key = file.runtime;
      if (!grouped[key]) {
        grouped[key] = [];
      }
      grouped[key].push(file);
    }
    return grouped;
  };

  const renderFileItem = (file: AgentConfigFile) => {
    const Icon = FILE_TYPE_ICONS[file.fileType] || FileText;
    const isSelected = selectedFilePath === file.path;
    const fileName = file.path.split("/").pop() || file.path;

    return (
      <button
        key={file.path}
        onClick={() => selectFile(file.path)}
        className={cn(
          "w-full flex items-center gap-2 px-3 py-1.5 text-sm rounded-md transition-colors text-left",
          isSelected
            ? "bg-cs-accent/15 text-cs-accent"
            : "text-cs-muted hover:text-cs-text hover:bg-cs-border/50",
          !file.exists && "opacity-50"
        )}
      >
        <Icon size={14} className={RUNTIME_COLORS[file.runtime]} />
        <span className="flex-1 truncate">{fileName}</span>
        {file.tokenCount && (
          <span className="text-xs text-cs-muted">{formatTokens(file.tokenCount)}</span>
        )}
        {!file.exists && (
          <span className="text-xs text-cs-muted italic">
            {t("agentManager.notFound", "not found")}
          </span>
        )}
      </button>
    );
  };

  const renderSection = (
    title: string,
    icon: typeof Globe,
    files: AgentConfigFile[],
    sectionKey: string
  ) => {
    const Icon = icon;
    const isExpanded = expandedSections[sectionKey];
    const groupedFiles = groupFilesByRuntime(files);
    const runtimes = Object.keys(groupedFiles).sort();

    return (
      <div className="mb-2">
        <button
          onClick={() => toggleSection(sectionKey)}
          className="w-full flex items-center gap-2 px-3 py-2 text-sm font-medium text-cs-text hover:bg-cs-border/30 rounded-md transition-colors"
        >
          {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          <Icon size={14} />
          <span className="flex-1 text-left">{title}</span>
          <span className="text-xs text-cs-muted">
            {files.filter((f) => f.exists).length} files
          </span>
        </button>

        {isExpanded && (
          <div className="ml-2 mt-1 space-y-2">
            {runtimes.map((runtime) => (
              <div key={runtime}>
                <div className="flex items-center gap-2 px-3 py-1 text-xs font-medium text-cs-muted uppercase tracking-wide">
                  <span className={RUNTIME_COLORS[runtime]}>{runtime}</span>
                </div>
                <div className="space-y-0.5">
                  {groupedFiles[runtime].map(renderFileItem)}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    );
  };

  if (isLoading) {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="animate-pulse text-cs-muted">
          {t("common.loading", "Loading...")}
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-cs-card">
      {/* Header */}
      <div className="px-3 py-2 border-b border-cs-border">
        <h3 className="text-sm font-medium">
          {t("agentManager.explorer.title", "Config Files")}
        </h3>
        {activeRuntime !== "all" && (
          <p className="text-xs text-cs-muted mt-0.5">
            Filtered: {activeRuntime}
          </p>
        )}
      </div>

      {/* File tree */}
      <div className="flex-1 overflow-y-auto p-2">
        {renderSection(
          t("agentManager.explorer.global", "Global (~/)"),
          Globe,
          globalFiles,
          "global"
        )}
        {renderSection(
          t("agentManager.explorer.project", "Project"),
          FolderOpen,
          projectFiles,
          "project"
        )}

        {globalFiles.length === 0 && projectFiles.length === 0 && (
          <div className="text-center text-cs-muted py-8">
            <FileText size={32} className="mx-auto mb-2 opacity-50" />
            <p className="text-sm">
              {t("agentManager.explorer.noFiles", "No config files found")}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

function formatTokens(tokens: number): string {
  if (tokens >= 1000) {
    return `${(tokens / 1000).toFixed(1)}k`;
  }
  return String(tokens);
}
