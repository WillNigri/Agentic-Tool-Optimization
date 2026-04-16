import { useState, useMemo, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  BookOpen,
  Bot,
  FileText,
  Loader2,
  Settings2,
  Sparkles,
  Webhook,
  ShieldCheck,
  Server,
  AlertTriangle,
  Terminal,
  Box,
  ShieldAlert,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { getProjectBundle, type Project } from "@/lib/api";
import { useProjectStore, type ProjectSection } from "@/stores/useProjectStore";
import FileViewer from "@/components/FileViewer";
import FileRefList from "./sections/FileRefList";
import SectionShell from "./sections/SectionShell";
import SkillsSection from "./sections/SkillsSection";
import HooksSection from "./sections/HooksSection";
import PermissionsSection from "./sections/PermissionsSection";
import McpSection from "./sections/McpSection";
import RuntimeSkillsSection from "./sections/RuntimeSkillsSection";
import SandboxSection from "./sections/SandboxSection";
import ApprovalPoliciesSection from "./sections/ApprovalPoliciesSection";
import CodexConfigView from "./sections/CodexConfigView";
import OpenClawWorkspaceView from "./sections/OpenClawWorkspaceView";
import GeminiAgentView from "./sections/GeminiAgentView";
import ProjectActivityFeed from "./ProjectActivityFeed";
import TokenBreakdownChart from "./TokenBreakdownChart";

interface ProjectDashboardProps {
  project: Project;
  onBack: () => void;
}

type Runtime = "claude" | "codex" | "openclaw" | "hermes" | "gemini";

const CLAUDE_SECTIONS: Array<{ id: ProjectSection; label: string; icon: typeof BookOpen }> = [
  { id: "memory", label: "Memory", icon: BookOpen },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "subagents", label: "Subagents", icon: Bot },
  { id: "commands", label: "Commands", icon: Terminal },
  { id: "hooks", label: "Hooks", icon: Webhook },
  { id: "permissions", label: "Permissions", icon: ShieldCheck },
  { id: "mcp", label: "MCP", icon: Server },
];

const CODEX_SECTIONS: Array<{ id: ProjectSection; label: string; icon: typeof BookOpen }> = [
  { id: "memory", label: "Files", icon: FileText },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "sandbox", label: "Sandbox", icon: Box },
  { id: "policies", label: "Policies", icon: ShieldAlert },
];

const OTHER_SECTIONS: Array<{ id: ProjectSection; label: string; icon: typeof BookOpen }> = [
  { id: "memory", label: "Files", icon: FileText },
  { id: "skills", label: "Skills", icon: Sparkles },
];

const RUNTIME_META: Record<Runtime, { label: string; color: "orange" | "green" | "purple" | "cyan" | "blue" }> = {
  claude: { label: "Claude", color: "orange" },
  codex: { label: "Codex", color: "green" },
  hermes: { label: "Hermes", color: "purple" },
  openclaw: { label: "OpenClaw", color: "cyan" },
  gemini: { label: "Gemini", color: "blue" },
};

export default function ProjectDashboard({ project, onBack }: ProjectDashboardProps) {
  const { t } = useTranslation();
  const { selectedSection, setSelectedSection } = useProjectStore();
  const [openFile, setOpenFile] = useState<string | null>(null);

  const detectedRuntimes: Runtime[] = [
    project.hasClaude && "claude",
    project.hasCodex && "codex",
    project.hasOpenclaw && "openclaw",
    project.hasHermes && "hermes",
    project.hasGemini && "gemini",
  ].filter(Boolean) as Runtime[];

  const [activeRuntime, setActiveRuntime] = useState<Runtime>(
    detectedRuntimes[0] ?? "claude"
  );

  const sections = activeRuntime === "claude"
    ? CLAUDE_SECTIONS
    : activeRuntime === "codex"
    ? CODEX_SECTIONS
    : OTHER_SECTIONS;

  // If the user switches runtime to one whose section list doesn't have the
  // currently selected section, fall back to the first valid section.
  const currentSectionValid = sections.some((s) => s.id === selectedSection);

  const queryClient = useQueryClient();

  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["project-bundle", project.path],
    queryFn: () => getProjectBundle(project.path),
    staleTime: 10_000,
  });

  // File watcher — start on mount, stop on unmount
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { listen } = await import("@tauri-apps/api/event");
        await invoke("watch_project_files", { projectPath: project.path });
        unlisten = await listen("project-files-changed", () => {
          queryClient.invalidateQueries({ queryKey: ["project-bundle", project.path] });
        });
      } catch {}
    })();
    return () => {
      unlisten?.();
      import("@tauri-apps/api/core").then(({ invoke }) => {
        invoke("stop_watching_project", { projectPath: project.path }).catch(() => {});
      }).catch(() => {});
    };
  }, [project.path, queryClient]);

  const [showEmptySections, setShowEmptySections] = useState(false);

  const sectionHasContent = useMemo<Record<string, boolean>>(() => {
    if (!data || activeRuntime !== "claude") return {};
    return {
      memory: data.memoryFiles.some((f) => f.exists),
      skills: data.skills.length > 0,
      subagents: data.subagents.some((f) => f.exists),
      commands: data.commands.some((f) => f.exists),
      hooks: data.hooks.length > 0,
      permissions: data.permissionsUser.allow.length + data.permissionsUser.deny.length + data.permissionsUser.ask.length +
        data.permissionsProject.allow.length + data.permissionsProject.deny.length + data.permissionsProject.ask.length > 0,
      mcp: data.mcpServers.length > 0,
    };
  }, [data, activeRuntime]);

  const populatedSections = activeRuntime === "claude"
    ? sections.filter((s) => sectionHasContent[s.id] !== false)
    : sections;
  const emptySections = activeRuntime === "claude"
    ? sections.filter((s) => sectionHasContent[s.id] === false)
    : [];

  return (
    <div className="flex h-full flex-col bg-cs-bg">
      {/* Breadcrumb / header */}
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-cs-border pb-4">
        <div className="flex items-start gap-3">
          <button
            onClick={onBack}
            className="mt-0.5 flex items-center gap-1.5 rounded-md border border-cs-border px-2.5 py-1 text-xs text-cs-muted transition-colors hover:bg-cs-border/50 hover:text-cs-text"
          >
            <ArrowLeft size={12} /> {t("projects.backToProjects", "Projects")}
          </button>
          <div className="min-w-0">
            <h1 className="truncate text-lg font-semibold">{project.name}</h1>
            <p className="mt-0.5 truncate font-mono text-[11px] text-cs-muted">{project.path}</p>
          </div>
        </div>

        <div className="flex flex-wrap gap-1.5">
          <RuntimePill active={project.hasClaude} label="Claude" color="orange" />
          <RuntimePill active={project.hasCodex} label="Codex" color="green" />
          <RuntimePill active={project.hasHermes} label="Hermes" color="purple" />
          <RuntimePill active={project.hasOpenclaw} label="OpenClaw" color="cyan" />
          <RuntimePill active={project.hasGemini} label="Gemini" color="blue" />
        </div>
      </div>

      {/* Runtime switcher (only if more than one runtime is detected) */}
      {detectedRuntimes.length > 1 && (
        <nav className="flex gap-1 overflow-x-auto border-b border-cs-border py-2">
          {detectedRuntimes.map((rt) => {
            const meta = RUNTIME_META[rt];
            const active = activeRuntime === rt;
            return (
              <button
                key={rt}
                onClick={() => setActiveRuntime(rt)}
                className={cn(
                  "rounded-md border px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap",
                  active
                    ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
                    : "border-cs-border text-cs-muted hover:bg-cs-border/50 hover:text-cs-text"
                )}
              >
                {meta.label}
              </button>
            );
          })}
        </nav>
      )}

      {/* Section tabs */}
      <nav className="flex flex-wrap gap-1 border-b border-cs-border py-2">
        {populatedSections.map((s) => {
          const active = currentSectionValid ? selectedSection === s.id : s === populatedSections[0];
          return (
            <button
              key={s.id}
              onClick={() => setSelectedSection(s.id)}
              className={cn(
                "flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap",
                active
                  ? "bg-cs-accent/10 text-cs-accent"
                  : "text-cs-muted hover:bg-cs-border/50 hover:text-cs-text"
              )}
            >
              <s.icon size={13} />
              {s.label}
            </button>
          );
        })}
        {emptySections.length > 0 && (
          <>
            <button
              onClick={() => setShowEmptySections((v) => !v)}
              className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[10px] text-cs-muted/60 transition-colors hover:text-cs-muted whitespace-nowrap"
            >
              {showEmptySections ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
              +{emptySections.length} more
            </button>
            {showEmptySections && emptySections.map((s) => (
              <button
                key={s.id}
                onClick={() => setSelectedSection(s.id)}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap",
                  selectedSection === s.id
                    ? "bg-cs-accent/10 text-cs-accent"
                    : "text-cs-muted/50 hover:bg-cs-border/50 hover:text-cs-muted"
                )}
              >
                <s.icon size={13} />
                {s.label}
              </button>
            ))}
          </>
        )}
      </nav>

      {/* Body */}
      <div className="flex-1 overflow-y-auto py-4">
        {isLoading && (
          <div className="flex h-60 items-center justify-center gap-2 text-xs text-cs-muted">
            <Loader2 size={14} className="animate-spin" /> Loading project bundle…
          </div>
        )}
        {isError && (
          <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-4 text-xs text-red-300">
            <div className="flex items-center gap-2">
              <AlertTriangle size={14} />
              Failed to load project: {error instanceof Error ? error.message : String(error)}
            </div>
          </div>
        )}
        {data && activeRuntime === "claude" && (
          <div className="space-y-4">
            {selectedSection === "memory" && (
              <>
                <TokenBreakdownChart bundle={data} />
                <SectionShell
                  icon={BookOpen}
                  title={t("projects.memory", "Memory")}
                  subtitle={t("projects.memorySubtitle", "CLAUDE.md files Claude reads on start — hierarchy: user \u2192 project \u2192 nested")}
                  count={data.memoryFiles.filter((f) => f.exists).length}
                >
                  <FileRefList
                    files={data.memoryFiles}
                    onOpen={setOpenFile}
                    emptyMessage="No CLAUDE.md files found."
                  />
                </SectionShell>
              </>
            )}

            {selectedSection === "skills" && (
              <SkillsSection skills={data.skills} onOpen={setOpenFile} />
            )}

            {selectedSection === "subagents" && (
              <SectionShell
                icon={Bot}
                title={t("projects.subagents", "Subagents")}
                subtitle={t("projects.subagentsSubtitle", ".claude/agents/*.md (global + project)")}
                count={data.subagents.filter((f) => f.exists).length}
              >
                <FileRefList
                  files={data.subagents}
                  onOpen={setOpenFile}
                  emptyMessage="No subagents defined."
                />
              </SectionShell>
            )}

            {selectedSection === "commands" && (
              <SectionShell
                icon={Terminal}
                title={t("projects.commands", "Commands")}
                subtitle={t("projects.commandsSubtitle", ".claude/commands/*.md (global + project)")}
                count={data.commands.filter((f) => f.exists).length}
              >
                <FileRefList
                  files={data.commands}
                  onOpen={setOpenFile}
                  emptyMessage="No slash commands defined."
                />
              </SectionShell>
            )}

            {selectedSection === "hooks" && (
              <HooksSection
                hooks={data.hooks}
                onOpenFile={setOpenFile}
                settingsPath={data.settingsFiles.find((f) => f.scope === "project" && f.exists)?.path
                  ?? data.settingsFiles.find((f) => f.scope === "user" && f.exists)?.path}
              />
            )}

            {selectedSection === "permissions" && (
              <PermissionsSection user={data.permissionsUser} project={data.permissionsProject} />
            )}

            {selectedSection === "mcp" && (
              <McpSection
                servers={data.mcpServers}
                onCreateMcpJson={() => {
                  const mcpPath = project.path + "/.mcp.json";
                  import("@/lib/tauri-api").then((api) =>
                    api.writeAgentConfigFile(mcpPath, '{\n  "mcpServers": {}\n}\n', { skipValidation: true })
                      .then(() => setOpenFile(mcpPath))
                  );
                }}
              />
            )}

            {/* Source-of-truth settings files beneath config-derived sections */}
            {(selectedSection === "hooks" || selectedSection === "permissions" || selectedSection === "mcp") && (
              <SectionShell
                icon={Settings2}
                title={t("projects.settingsFiles", "Settings files")}
                subtitle={t("projects.settingsFilesSubtitle", "Source of truth for hooks, permissions, and MCP")}
                count={data.settingsFiles.filter((f) => f.exists).length}
              >
                <FileRefList
                  files={data.settingsFiles}
                  onOpen={setOpenFile}
                  emptyMessage="No settings files yet."
                />
              </SectionShell>
            )}
          </div>
        )}

        {data && activeRuntime === "codex" && (
          <div className="space-y-4">
            {(selectedSection === "memory" || !currentSectionValid) && (
              <>
                {data.codexFiles.some((f) => f.label.includes("config.toml") && f.exists) && (
                  <CodexConfigView
                    configPath={data.codexFiles.find((f) => f.label.includes("config.toml") && f.exists)!.path}
                    onOpenRaw={setOpenFile}
                  />
                )}
              <SectionShell
                icon={FileText}
                title="Codex Files"
                subtitle="AGENTS.md and config.toml — user + project"
                count={data.codexFiles.filter((f) => f.exists).length}
              >
                <FileRefList
                  files={data.codexFiles}
                  onOpen={setOpenFile}
                  emptyMessage="No Codex files found."
                />
              </SectionShell>
              </>
            )}
            {selectedSection === "skills" && (
              <RuntimeSkillsSection runtime="codex" skills={data.codexSkills} onOpen={setOpenFile} />
            )}
            {selectedSection === "sandbox" && (
              <SandboxSection
                config={data.sandboxConfig}
                projectPath={project.path}
                onOpenSource={setOpenFile}
                onCreate={() => {
                  const sandboxPath = project.path + "/.codex/sandbox.json";
                  const defaults = JSON.stringify({
                    sandbox: { enabled: true, network_isolation: true, filesystem_policy: "read-only", timeout_secs: 300, snapshot_enabled: false, allowed_ports: [] }
                  }, null, 2);
                  import("@/lib/tauri-api").then((api) =>
                    api.writeAgentConfigFile(sandboxPath, defaults + "\n", { skipValidation: true })
                      .then(() => setOpenFile(sandboxPath))
                  );
                }}
              />
            )}
            {selectedSection === "policies" && (
              <ApprovalPoliciesSection
                policies={data.approvalPolicies}
                projectPath={project.path}
                onCreate={() => {
                  const policiesPath = project.path + "/.codex/policies.json";
                  const template = JSON.stringify({ approvalPolicies: {} }, null, 2);
                  import("@/lib/tauri-api").then((api) =>
                    api.writeAgentConfigFile(policiesPath, template + "\n", { skipValidation: true })
                      .then(() => setOpenFile(policiesPath))
                  );
                }}
              />
            )}
          </div>
        )}

        {data && activeRuntime === "openclaw" && (
          <div className="space-y-4">
            {(selectedSection === "memory" || !currentSectionValid) && (
              <>
                <OpenClawWorkspaceView projectPath={project.path} onOpenFile={setOpenFile} />
                <SectionShell
                  icon={FileText}
                  title="OpenClaw Files"
                  subtitle="SOUL.md, TOOLS.md, AGENTS.md, openclaw.json"
                  count={data.openclawFiles.filter((f) => f.exists).length}
                >
                  <FileRefList
                    files={data.openclawFiles}
                    onOpen={setOpenFile}
                    emptyMessage="No OpenClaw files found."
                  />
                </SectionShell>
              </>
            )}
            {selectedSection === "skills" && (
              <RuntimeSkillsSection runtime="openclaw" skills={data.openclawSkills} onOpen={setOpenFile} />
            )}
          </div>
        )}

        {data && activeRuntime === "hermes" && (
          <div className="space-y-4">
            {(selectedSection === "memory" || !currentSectionValid) && (
              <SectionShell
                icon={FileText}
                title="Hermes Files"
                subtitle="SOUL.md, memories/MEMORY.md, memories/USER.md, config.yaml"
                count={data.hermesFiles.filter((f) => f.exists).length}
              >
                <FileRefList
                  files={data.hermesFiles}
                  onOpen={setOpenFile}
                  emptyMessage="No Hermes files found."
                />
              </SectionShell>
            )}
            {selectedSection === "skills" && (
              <RuntimeSkillsSection runtime="hermes" skills={data.hermesSkills} onOpen={setOpenFile} />
            )}
          </div>
        )}

        {data && activeRuntime === "gemini" && (
          <div className="space-y-4">
            {(selectedSection === "memory" || !currentSectionValid) && (
              <>
                {data.geminiFiles.some((f) => f.label === "root_agent.yaml" && f.exists) && (
                  <GeminiAgentView
                    agentPath={data.geminiFiles.find((f) => f.label === "root_agent.yaml")!.path}
                    onOpenFile={setOpenFile}
                  />
                )}
                <SectionShell
                  icon={FileText}
                  title="Gemini Files"
                  subtitle="GEMINI.md, settings.json, root_agent.yaml"
                  count={data.geminiFiles.filter((f) => f.exists).length}
                >
                  <FileRefList
                    files={data.geminiFiles}
                    onOpen={setOpenFile}
                    emptyMessage="No Gemini files found."
                  />
                </SectionShell>
              </>
            )}
            {selectedSection === "skills" && (
              <RuntimeSkillsSection runtime="hermes" skills={data.geminiSkills} onOpen={setOpenFile} />
            )}
          </div>
        )}

        {data && (
          <div className="mt-4">
            <ProjectActivityFeed projectPath={project.path} />
          </div>
        )}
      </div>

      {openFile && (
        <FileViewer
          filePath={openFile}
          onClose={() => setOpenFile(null)}
        />
      )}
    </div>
  );
}

function RuntimePill({
  active,
  label,
  color,
}: {
  active: boolean;
  label: string;
  color: "orange" | "green" | "purple" | "cyan" | "blue";
}) {
  const colors: Record<string, string> = {
    orange: "text-orange-300 bg-orange-500/10 border-orange-500/20",
    green: "text-green-300 bg-green-500/10 border-green-500/20",
    purple: "text-purple-300 bg-purple-500/10 border-purple-500/20",
    cyan: "text-cyan-300 bg-cyan-500/10 border-cyan-500/20",
    blue: "text-blue-300 bg-blue-500/10 border-blue-500/20",
  };
  return (
    <span
      className={cn(
        "rounded border px-2 py-0.5 text-[10px] font-medium",
        active ? colors[color] : "border-cs-border/60 text-cs-muted/40"
      )}
    >
      {label}
    </span>
  );
}
