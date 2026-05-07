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
import type { AgentConfigFile } from "@/lib/api";
import type { AgentConfigScope } from "@/lib/tauri-api";

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

  const renderFileItem = (file: AgentConfigFile, opts: { hideProjectPrefix?: boolean } = {}) => {
    const Icon = FILE_TYPE_ICONS[file.fileType] || FileText;
    const isSelected = selectedFilePath === file.path;
    const fileName = file.path.split("/").pop() || file.path;
    // VS-Code-style label: when a project file is rendered outside a
    // project sub-group, prefix with the project folder name so users
    // with multiple projects can tell `aluminaria-sky/CLAUDE.md` apart
    // from `cambã/CLAUDE.md` at a glance.
    const showPrefix = !opts.hideProjectPrefix && file.scope === "project" && file.projectName;
    const displayName = showPrefix ? `${file.projectName} / ${fileName}` : fileName;

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
        title={file.path}
      >
        <Icon size={14} className={RUNTIME_COLORS[file.runtime]} />
        <span className="flex-1 truncate">{displayName}</span>
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

  // Group project files by their projectName so VS-Code-style folders
  // appear: each project gets a sub-header with its folder name and the
  // files indented underneath. Avoids mixing files from different projects.
  const groupFilesByProject = (files: AgentConfigFile[]) => {
    const grouped: Record<string, AgentConfigFile[]> = {};
    for (const file of files) {
      const key = file.projectName ?? "(unknown project)";
      if (!grouped[key]) grouped[key] = [];
      grouped[key].push(file);
    }
    return grouped;
  };

  const renderSection = (
    title: string,
    icon: typeof Globe,
    files: AgentConfigFile[],
    sectionKey: string,
    scope: AgentConfigScope = "global"
  ) => {
    const Icon = icon;
    const isExpanded = expandedSections[sectionKey];

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
            {scope === "project" ? (
              // Project scope — group by project folder name first.
              Object.entries(groupFilesByProject(files))
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([projectName, projectFiles]) => (
                  <div key={projectName}>
                    <div className="flex items-center gap-1 px-3 py-1 text-xs font-medium text-cs-text">
                      <FolderOpen size={11} className="text-cs-muted" />
                      <span className="truncate">{projectName}</span>
                      <span className="text-cs-muted ml-auto">
                        {projectFiles.filter((f) => f.exists).length}
                      </span>
                    </div>
                    <div className="space-y-0.5 ml-2">
                      {projectFiles.map((f) => renderFileItem(f, { hideProjectPrefix: true }))}
                    </div>
                  </div>
                ))
            ) : (
              // Global scope — keep the runtime-grouped layout.
              Object.entries(groupFilesByRuntime(files))
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([runtime, runtimeFiles]) => (
                  <div key={runtime}>
                    <div className="flex items-center gap-2 px-3 py-1 text-xs font-medium text-cs-muted uppercase tracking-wide">
                      <span className={RUNTIME_COLORS[runtime]}>{runtime}</span>
                    </div>
                    <div className="space-y-0.5">
                      {runtimeFiles.map((f) => renderFileItem(f))}
                    </div>
                  </div>
                ))
            )}
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
          "global",
          "global"
        )}
        {renderSection(
          t("agentManager.explorer.project", "Project"),
          FolderOpen,
          projectFiles,
          "project",
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
