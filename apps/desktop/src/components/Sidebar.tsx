import { useState } from "react";
import {
  Layers,
  Sparkles,
  BarChart3,
  Server,
  Settings,
  LogOut,
  Bot,
  Webhook,
  Workflow,
  Clock,
  Crown,
  User,
  Settings2,
  FolderKanban,
  KeyRound,
  FileCode,
  Cpu,
  ScrollText,
  Activity,
  Cloud,
  Users,
  RefreshCw,
  BellRing,
  Shield,
  Key,
  MonitorDot,
  ChevronDown,
  ChevronRight,
  Loader2,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/hooks/useAuth";
import { useTranslation } from "react-i18next";
import { useCronStore } from "@/stores/useCronStore";
import { useProjectStore } from "@/stores/useProjectStore";
import { listProjects } from "@/lib/api";
import LoginModal from "./LoginModal";

export type Section = "context" | "skills" | "projects" | "subagents" | "hooks" | "automation" | "cron" | "analytics" | "mcp" | "agents" | "config" | "secrets" | "env" | "models" | "logs" | "health" | "cloud" | "teams" | "sync" | "notifications" | "audit" | "llm-keys" | "agent-monitor";

interface SidebarProps {
  active: Section;
  onNavigate: (section: Section) => void;
}

const NAV_ITEMS: { id: Section; labelKey: string; icon: typeof Layers; group?: string }[] = [
  { id: "context", labelKey: "nav.context", icon: Layers },
  { id: "skills", labelKey: "nav.skills", icon: Sparkles },
  { id: "projects", labelKey: "nav.projects", icon: FolderKanban },
  { id: "subagents", labelKey: "nav.subagents", icon: Bot },
  { id: "hooks", labelKey: "nav.hooks", icon: Webhook },
  { id: "automation", labelKey: "nav.automation", icon: Workflow },
  { id: "cron", labelKey: "nav.cron", icon: Clock },
  { id: "analytics", labelKey: "nav.analytics", icon: BarChart3 },
  { id: "logs", labelKey: "nav.logs", icon: ScrollText },
  { id: "health", labelKey: "nav.health", icon: Activity },
  { id: "mcp", labelKey: "nav.mcp", icon: Server },
  { id: "agents", labelKey: "nav.agents", icon: Settings2 },
  { id: "cloud", labelKey: "nav.cloud", icon: Cloud },
  { id: "teams", labelKey: "nav.teams", icon: Users },
  { id: "sync", labelKey: "nav.sync", icon: RefreshCw },
  { id: "notifications", labelKey: "nav.notifications", icon: BellRing },
  { id: "agent-monitor", labelKey: "nav.agentMonitor", icon: MonitorDot },
  { id: "llm-keys", labelKey: "nav.llmKeys", icon: Key },
  { id: "audit", labelKey: "nav.audit", icon: Shield },
  { id: "secrets", labelKey: "nav.secrets", icon: KeyRound },
  { id: "env", labelKey: "nav.env", icon: FileCode },
  { id: "models", labelKey: "nav.models", icon: Cpu },
  { id: "config", labelKey: "nav.config", icon: Settings },
];

const LANGUAGES = [
  { code: "en", label: "EN" },
  { code: "pt", label: "PT" },
  { code: "es", label: "ES" },
] as const;

export default function Sidebar({ active, onNavigate }: SidebarProps) {
  const { t, i18n } = useTranslation();
  const logout = useAuthStore((s) => s.logout);
  const user = useAuthStore((s) => s.user);
  const cronAlertCount = useCronStore((s) => s.getActiveAlertCount());
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const [showLogin, setShowLogin] = useState(false);

  const activeProject = useProjectStore((s) => s.activeProject);
  const setActiveProjectStore = useProjectStore((s) => s.setActiveProject);
  const sidebarExpanded = useProjectStore((s) => s.sidebarExpanded);
  const toggleSidebarExpanded = useProjectStore((s) => s.toggleSidebarExpanded);

  const projectsInSidebar = active === "projects" && sidebarExpanded;
  const { data: projects = [], isLoading: projectsLoading } = useQuery({
    queryKey: ["projects"],
    queryFn: listProjects,
    enabled: projectsInSidebar,
    staleTime: 30_000,
  });

  function changeLanguage(lang: string) {
    i18n.changeLanguage(lang);
    localStorage.setItem("claudescope-lang", lang);
  }

  return (
    <aside className="w-56 h-screen bg-cs-card border-r border-cs-border flex flex-col shrink-0">
      <div className="px-4 py-5 border-b border-cs-border">
        <h1 className="text-lg font-bold tracking-tight">{t("app.name")}</h1>
        <p className="text-xs text-cs-muted mt-0.5 truncate">
          {user?.email}
        </p>
      </div>

      <nav className="flex-1 py-3 px-2 space-y-0.5 overflow-y-auto" aria-label="Main navigation" role="navigation">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const isActive = active === item.id;

          if (item.id === "projects") {
            return (
              <div key={item.id}>
                <div
                  className={cn(
                    "flex items-center gap-1 rounded-md text-sm transition-colors",
                    isActive
                      ? "bg-[#00FFB2]/15 text-[#00FFB2]"
                      : "text-cs-muted hover:text-cs-text hover:bg-cs-border/50"
                  )}
                >
                  <button
                    onClick={() => onNavigate(item.id)}
                    className="flex flex-1 items-center gap-3 px-3 py-2 text-left"
                  >
                    <Icon size={18} />
                    <span className="flex-1 text-left">{t(item.labelKey)}</span>
                  </button>
                  <button
                    onClick={() => toggleSidebarExpanded()}
                    title={sidebarExpanded ? "Collapse" : "Expand"}
                    className="p-2 text-current opacity-70 hover:opacity-100"
                  >
                    {sidebarExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  </button>
                </div>

                {sidebarExpanded && (
                  <div className="mt-1 ml-4 mb-1 border-l border-cs-border/60 pl-2 max-h-80 overflow-y-auto">
                    {projectsLoading ? (
                      <div className="flex items-center gap-2 px-2 py-1.5 text-[11px] text-cs-muted">
                        <Loader2 size={10} className="animate-spin" /> Loading…
                      </div>
                    ) : projects.length === 0 ? (
                      <p className="px-2 py-1.5 text-[11px] text-cs-muted">No projects yet.</p>
                    ) : (
                      projects.map((project) => {
                        const selected = activeProject?.id === project.id && active === "projects";
                        return (
                          <button
                            key={project.id}
                            onClick={() => {
                              setActiveProjectStore(project);
                              if (active !== "projects") onNavigate("projects");
                            }}
                            className={cn(
                              "flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] transition-colors",
                              selected
                                ? "bg-cs-accent/10 text-cs-accent"
                                : "text-cs-muted hover:bg-cs-border/50 hover:text-cs-text"
                            )}
                            title={project.path}
                          >
                            <span className="truncate flex-1">{project.name}</span>
                            <RuntimeDots project={project} />
                          </button>
                        );
                      })
                    )}
                  </div>
                )}
              </div>
            );
          }

          return (
            <button
              key={item.id}
              onClick={() => onNavigate(item.id)}
              className={cn(
                "w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors",
                isActive
                  ? "bg-[#00FFB2]/15 text-[#00FFB2]"
                  : "text-cs-muted hover:text-cs-text hover:bg-cs-border/50"
              )}
            >
              <Icon size={18} />
              <span className="flex-1 text-left">{t(item.labelKey)}</span>
              {item.id === "cron" && cronAlertCount > 0 && (
                <span className="w-2 h-2 rounded-full bg-red-500 animate-pulse" />
              )}
            </button>
          );
        })}
      </nav>

      <div className="px-2 pb-1">
        <div className="flex items-center gap-1 px-3 py-2">
          {LANGUAGES.map((lang) => (
            <button
              key={lang.code}
              onClick={() => changeLanguage(lang.code)}
              className={cn(
                "px-2 py-1 rounded text-xs font-medium transition-colors",
                i18n.language === lang.code
                  ? "text-[#00FFB2] bg-[#00FFB2]/15"
                  : "text-cs-muted hover:text-cs-text"
              )}
            >
              {lang.label}
            </button>
          ))}
        </div>
      </div>

      <div className="p-2 border-t border-cs-border space-y-1">
        {isCloudUser ? (
          <>
            <div className="flex items-center gap-2 px-3 py-2">
              <div className="w-7 h-7 rounded-full bg-cs-accent/10 border border-cs-accent/30 flex items-center justify-center">
                <User size={14} className="text-cs-accent" />
              </div>
              <div className="min-w-0">
                <p className="text-xs font-medium truncate">{user?.name || user?.email}</p>
                <p className="text-[10px] text-cs-accent flex items-center gap-1">
                  <Crown size={9} /> Pro
                </p>
              </div>
            </div>
            <button
              onClick={logout}
              className="w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm text-cs-muted hover:text-cs-danger hover:bg-cs-danger/10 transition-colors"
            >
              <LogOut size={18} />
              {t("nav.logout")}
            </button>
          </>
        ) : (
          <button
            onClick={() => setShowLogin(true)}
            className="w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm text-cs-accent hover:bg-cs-accent/10 transition-colors"
          >
            <Crown size={18} />
            Sign in for Pro
          </button>
        )}
      </div>

      {showLogin && <LoginModal onClose={() => setShowLogin(false)} />}
    </aside>
  );
}

function RuntimeDots({ project }: { project: { hasClaude: boolean; hasCodex: boolean; hasHermes: boolean; hasOpenclaw: boolean; hasGemini: boolean } }) {
  const runtimes = [
    { active: project.hasClaude, color: "bg-orange-400", title: "Claude" },
    { active: project.hasCodex, color: "bg-green-400", title: "Codex" },
    { active: project.hasHermes, color: "bg-purple-400", title: "Hermes" },
    { active: project.hasOpenclaw, color: "bg-cyan-400", title: "OpenClaw" },
    { active: project.hasGemini, color: "bg-blue-400", title: "Gemini" },
  ].filter((r) => r.active);

  if (runtimes.length === 0) return null;
  return (
    <span className="flex shrink-0 gap-0.5">
      {runtimes.map((r) => (
        <span
          key={r.title}
          className={cn("h-1.5 w-1.5 rounded-full", r.color)}
          title={r.title}
        />
      ))}
    </span>
  );
}
