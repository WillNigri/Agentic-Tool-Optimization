import { useState, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, LayoutGrid, Cpu, Sparkles, Server, BookOpen, RefreshCw } from "lucide-react";
import { getProjectBundle, listProjects } from "@/lib/api";
import type { ProjectBundle } from "@/lib/api";
import { useProjectStore } from "@/stores/useProjectStore";
import { cn } from "@/lib/utils";

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#f97316",
  codex: "#22c55e",
  gemini: "#3b82f6",
  openclaw: "#06b6d4",
  hermes: "#a855f7",
};

export default function WorkspaceView() {
  const activeProject = useProjectStore((s) => s.activeProject);

  const { data: projects = [] } = useQuery({
    queryKey: ["projects"],
    queryFn: listProjects,
    staleTime: 30_000,
  });

  const projectPath = activeProject?.path ?? projects[0]?.path;
  const projectName = activeProject?.name ?? projects[0]?.name;

  const { data: bundle, isLoading, refetch } = useQuery({
    queryKey: ["project-bundle", projectPath],
    queryFn: () => getProjectBundle(projectPath!),
    enabled: !!projectPath,
    staleTime: 10_000,
  });

  if (!projectPath) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-cs-bg p-6">
        <LayoutGrid size={48} className="text-cs-accent mb-4 opacity-30" />
        <h2 className="text-lg font-semibold mb-2">Agent Workspace</h2>
        <p className="text-sm text-cs-muted text-center max-w-md">
          Select a project in the Projects tab to see your agents, skills, and MCP servers here.
        </p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center bg-cs-bg">
        <Loader2 size={24} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  if (!bundle) {
    return (
      <div className="flex-1 flex items-center justify-center bg-cs-bg">
        <p className="text-sm text-cs-muted">Failed to load project data.</p>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-cs-bg">
      {/* Header */}
      <div className="flex items-center gap-3 px-6 py-3 border-b border-cs-border shrink-0">
        <LayoutGrid size={18} className="text-cs-accent" />
        <div className="flex-1 min-w-0">
          <h2 className="text-sm font-semibold">{projectName}</h2>
          <p className="text-[10px] text-cs-muted font-mono truncate">{projectPath}</p>
        </div>
        <button onClick={() => refetch()} className="p-1.5 rounded hover:bg-cs-border text-cs-muted hover:text-cs-text">
          <RefreshCw size={14} />
        </button>
      </div>

      {/* Workspace grid */}
      <div className="flex-1 overflow-y-auto p-6">
        {/* Runtimes row */}
        <h3 className="text-xs font-medium text-cs-muted uppercase tracking-wide mb-3">Runtimes</h3>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3 mb-8">
          <RuntimeCard name="Claude Code" runtime="claude" active={bundle.hasClaude} skillCount={bundle.skills.length} />
          <RuntimeCard name="Codex" runtime="codex" active={bundle.hasCodex} skillCount={bundle.codexSkills.length} />
          <RuntimeCard name="Gemini CLI" runtime="gemini" active={bundle.hasGemini} skillCount={bundle.geminiSkills.length} />
          <RuntimeCard name="OpenClaw" runtime="openclaw" active={bundle.hasOpenclaw} skillCount={bundle.openclawSkills.length} />
          <RuntimeCard name="Hermes" runtime="hermes" active={bundle.hasHermes} skillCount={bundle.hermesSkills.length} />
        </div>

        {/* Skills */}
        <h3 className="text-xs font-medium text-cs-muted uppercase tracking-wide mb-3">
          Skills ({bundle.skills.length + bundle.codexSkills.length + bundle.geminiSkills.length + bundle.openclawSkills.length + bundle.hermesSkills.length})
        </h3>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-2 mb-8">
          {[...bundle.skills, ...bundle.codexSkills, ...bundle.geminiSkills, ...bundle.openclawSkills, ...bundle.hermesSkills].map((skill) => (
            <div key={skill.id} className="rounded-lg border border-cs-border bg-cs-card px-3 py-2">
              <div className="flex items-center gap-2">
                <Sparkles size={12} style={{ color: RUNTIME_COLORS[skill.runtime] ?? "#00FFB2" }} />
                <span className="text-xs font-medium truncate">{skill.name}</span>
                <div className={cn("w-1.5 h-1.5 rounded-full ml-auto shrink-0", skill.enabled ? "bg-green-400" : "bg-gray-500")} />
              </div>
              {skill.description && <p className="text-[10px] text-cs-muted line-clamp-1 mt-0.5">{skill.description}</p>}
            </div>
          ))}
        </div>

        {/* MCP Servers */}
        {bundle.mcpServers.length > 0 && (
          <>
            <h3 className="text-xs font-medium text-cs-muted uppercase tracking-wide mb-3">
              MCP Servers ({bundle.mcpServers.length})
            </h3>
            <div className="grid grid-cols-2 md:grid-cols-3 gap-2 mb-8">
              {bundle.mcpServers.map((mcp, i) => (
                <div key={`${mcp.name}-${i}`} className="rounded-lg border border-cs-border bg-cs-card px-3 py-2">
                  <div className="flex items-center gap-2">
                    <Server size={12} className="text-blue-400" />
                    <span className="text-xs font-medium">{mcp.name}</span>
                    <span className="ml-auto rounded bg-blue-500/10 px-1.5 py-0.5 text-[9px] text-blue-300 uppercase">{mcp.kind}</span>
                  </div>
                  <p className="text-[10px] text-cs-muted truncate mt-0.5 font-mono">{mcp.commandOrUrl}</p>
                </div>
              ))}
            </div>
          </>
        )}

        {/* Memory files */}
        {bundle.memoryFiles.filter((f) => f.exists).length > 0 && (
          <>
            <h3 className="text-xs font-medium text-cs-muted uppercase tracking-wide mb-3">
              Memory ({bundle.memoryFiles.filter((f) => f.exists).length} files)
            </h3>
            <div className="grid grid-cols-2 md:grid-cols-3 gap-2">
              {bundle.memoryFiles.filter((f) => f.exists).map((f) => (
                <div key={f.path} className="rounded-lg border border-cs-border bg-cs-card px-3 py-2">
                  <div className="flex items-center gap-2">
                    <BookOpen size={12} className="text-yellow-400" />
                    <span className="text-xs font-medium truncate">{f.label}</span>
                  </div>
                  <p className="text-[10px] text-cs-muted mt-0.5">~{f.tokenEstimate.toLocaleString()} tokens</p>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function RuntimeCard({ name, runtime, active, skillCount }: { name: string; runtime: string; active: boolean; skillCount: number }) {
  const color = RUNTIME_COLORS[runtime] ?? "#666";
  return (
    <div className={cn(
      "rounded-xl border px-4 py-3 transition-all",
      active ? "border-opacity-40 bg-opacity-5" : "border-cs-border/30 opacity-40"
    )} style={{
      borderColor: active ? color : undefined,
      backgroundColor: active ? `${color}08` : undefined,
    }}>
      <div className="flex items-center gap-2 mb-1">
        <Cpu size={14} style={{ color }} />
        <span className="text-sm font-medium">{name}</span>
        <div className={cn("w-2 h-2 rounded-full ml-auto", active ? "bg-green-400" : "bg-gray-600")} />
      </div>
      <p className="text-[10px] text-cs-muted">{active ? `${skillCount} skills` : "Not detected"}</p>
    </div>
  );
}
