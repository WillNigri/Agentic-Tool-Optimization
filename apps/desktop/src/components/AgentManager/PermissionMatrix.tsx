import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, AlertTriangle, X, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAgentConfigStore } from "@/stores/useAgentConfigStore";
import { parseAgentPermissions, type AgentPermission } from "@/lib/tauri-api";

const TOOLS = [
  { name: "Bash", description: "Execute shell commands" },
  { name: "Read", description: "Read file contents" },
  { name: "Write", description: "Write to files" },
  { name: "Edit", description: "Edit existing files" },
  { name: "Glob", description: "Search file patterns" },
  { name: "Grep", description: "Search file contents" },
  { name: "WebFetch", description: "Fetch web content" },
  { name: "WebSearch", description: "Search the web" },
  { name: "Task", description: "Launch subagents" },
];

interface PermissionSource {
  name: string;
  path: string;
  permissions: AgentPermission[];
}

export default function PermissionMatrix() {
  const { t } = useTranslation();
  const [sources, setSources] = useState<PermissionSource[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const { configFiles, activeRuntime } = useAgentConfigStore();

  // Load permissions from all settings files
  useEffect(() => {
    const loadPermissions = async () => {
      setIsLoading(true);
      const settingsFiles = configFiles.filter(
        (f) =>
          f.fileType === "settings" &&
          f.exists &&
          (activeRuntime === "all" || f.runtime === activeRuntime)
      );

      const loadedSources: PermissionSource[] = [];

      for (const file of settingsFiles) {
        const permissions = await parseAgentPermissions(file.path);
        loadedSources.push({
          name: `${file.scope} (${file.runtime})`,
          path: file.path,
          permissions,
        });
      }

      // Also check for skill-specific permissions
      const skillFiles = configFiles.filter(
        (f) =>
          f.fileType === "skill" &&
          f.exists &&
          (activeRuntime === "all" || f.runtime === activeRuntime)
      );

      for (const file of skillFiles) {
        // Skills have permissions in frontmatter, mock for now
        const skillName = file.path.split("/").pop()?.replace(".md", "") || "skill";
        loadedSources.push({
          name: skillName,
          path: file.path,
          permissions: [], // Would parse from frontmatter
        });
      }

      setSources(loadedSources);
      setIsLoading(false);
    };

    loadPermissions();
  }, [configFiles, activeRuntime]);

  const getPermissionStatus = (
    tool: string,
    source: PermissionSource
  ): "allowed" | "denied" | "approval" | "none" => {
    const perm = source.permissions.find((p) => p.tool === tool);
    if (!perm) return "none";
    if (!perm.allowed) return "denied";
    if (perm.requiresApproval) return "approval";
    return "allowed";
  };

  const renderCell = (status: "allowed" | "denied" | "approval" | "none") => {
    switch (status) {
      case "allowed":
        return (
          <div className="w-6 h-6 rounded bg-green-500/20 flex items-center justify-center">
            <Check size={14} className="text-green-400" />
          </div>
        );
      case "denied":
        return (
          <div className="w-6 h-6 rounded bg-red-500/20 flex items-center justify-center">
            <X size={14} className="text-red-400" />
          </div>
        );
      case "approval":
        return (
          <div className="w-6 h-6 rounded bg-yellow-500/20 flex items-center justify-center">
            <AlertTriangle size={14} className="text-yellow-400" />
          </div>
        );
      default:
        return <div className="w-6 h-6 rounded bg-cs-border/50" />;
    }
  };

  if (isLoading) {
    return (
      <div className="h-full flex items-center justify-center">
        <RefreshCw size={24} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto">
      <div className="min-w-[600px]">
        {/* Legend */}
        <div className="flex items-center gap-6 mb-4 text-sm">
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-green-500/20 flex items-center justify-center">
              <Check size={10} className="text-green-400" />
            </div>
            <span className="text-cs-muted">
              {t("agentManager.permissions.allowed", "Always allowed")}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-yellow-500/20 flex items-center justify-center">
              <AlertTriangle size={10} className="text-yellow-400" />
            </div>
            <span className="text-cs-muted">
              {t("agentManager.permissions.approval", "Requires approval")}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-red-500/20 flex items-center justify-center">
              <X size={10} className="text-red-400" />
            </div>
            <span className="text-cs-muted">
              {t("agentManager.permissions.denied", "Denied")}
            </span>
          </div>
        </div>

        {/* Matrix table */}
        <div className="border border-cs-border rounded-lg overflow-hidden">
          <table className="w-full">
            <thead>
              <tr className="bg-cs-card">
                <th className="text-left px-4 py-3 text-sm font-medium border-b border-cs-border">
                  {t("agentManager.permissions.tool", "Tool")}
                </th>
                {sources.map((source) => (
                  <th
                    key={source.path}
                    className="px-3 py-3 text-sm font-medium border-b border-cs-border text-center"
                  >
                    <div className="truncate max-w-[100px]" title={source.path}>
                      {source.name}
                    </div>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {TOOLS.map((tool, idx) => (
                <tr
                  key={tool.name}
                  className={cn(
                    "hover:bg-cs-border/30 transition-colors",
                    idx % 2 === 0 ? "bg-transparent" : "bg-cs-card/50"
                  )}
                >
                  <td className="px-4 py-2 border-b border-cs-border/50">
                    <div className="text-sm font-medium">{tool.name}</div>
                    <div className="text-xs text-cs-muted">{tool.description}</div>
                  </td>
                  {sources.map((source) => (
                    <td
                      key={`${tool.name}-${source.path}`}
                      className="px-3 py-2 border-b border-cs-border/50 text-center"
                    >
                      <div className="flex justify-center">
                        {renderCell(getPermissionStatus(tool.name, source))}
                      </div>
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {sources.length === 0 && (
          <div className="text-center text-cs-muted py-12">
            <AlertTriangle size={32} className="mx-auto mb-3 opacity-50" />
            <p>{t("agentManager.permissions.noSources", "No permission sources found")}</p>
            <p className="text-sm mt-1">
              {t(
                "agentManager.permissions.noSourcesHint",
                "Create a settings.json file to configure permissions"
              )}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
