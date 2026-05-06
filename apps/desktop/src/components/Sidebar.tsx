import { useState } from "react";
import {
  Home as HomeIcon,
  Bot,
  Sparkles,
  Activity,
  BarChart3,
  Settings,
  LogOut,
  Crown,
  User,
  ChevronDown,
  ChevronRight,
  Loader2,
  FolderKanban,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/hooks/useAuth";
import { useTranslation } from "react-i18next";
import { useCronStore } from "@/stores/useCronStore";
import { useProjectStore } from "@/stores/useProjectStore";
import { listProjects } from "@/lib/api";
import LoginModal from "./LoginModal";

// v1.3.0 — IA collapse from 24 entries to 6 top-level sections (T1).
// Sub-tabs are owned by each section component under pages/sections/*.

export type Section = "home" | "agents" | "skills" | "runs" | "insights" | "settings";

interface SidebarProps {
  active: Section;
  onNavigate: (section: Section) => void;
}

const NAV_ITEMS: { id: Section; labelKey: string; icon: typeof HomeIcon }[] = [
  { id: "home", labelKey: "nav.home", icon: HomeIcon },
  { id: "agents", labelKey: "nav.agents", icon: Bot },
  { id: "skills", labelKey: "nav.skills", icon: Sparkles },
  { id: "runs", labelKey: "nav.runs", icon: Activity },
  { id: "insights", labelKey: "nav.insights", icon: BarChart3 },
  { id: "settings", labelKey: "nav.settings", icon: Settings },
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

  const { data: projects = [], isLoading: projectsLoading } = useQuery({
    queryKey: ["projects"],
    queryFn: listProjects,
    enabled: sidebarExpanded,
    staleTime: 30_000,
  });

  function changeLanguage(lang: string) {
    i18n.changeLanguage(lang);
    localStorage.setItem("claudescope-lang", lang);
  }

  return (
    <aside className="w-56 h-screen bg-cs-card border-r border-cs-border flex flex-col shrink-0">
      {/* Header */}
      <div className="px-4 py-5 border-b border-cs-border">
        <h1 className="text-lg font-bold tracking-tight">{t("app.name")}</h1>
        <p className="text-xs text-cs-muted mt-0.5 truncate">{user?.email}</p>
      </div>

      {/* Sticky project switcher */}
      <div className="px-2 pt-2">
        <div
          className={cn(
            "rounded-md text-sm transition-colors",
            "text-cs-muted hover:text-cs-text"
          )}
        >
          <button
            onClick={() => toggleSidebarExpanded()}
            className="w-full flex items-center gap-2 px-3 py-2 rounded-md hover:bg-cs-border/50 text-left"
            title={t("sidebar.projectSwitcher", "Switch project")}
          >
            <FolderKanban size={16} />
            <span className="flex-1 truncate text-xs uppercase tracking-wide">
              {activeProject?.name ?? t("sidebar.noProject", "No project")}
            </span>
            {sidebarExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          </button>

          {sidebarExpanded && (
            <div className="mt-1 ml-2 mb-2 border-l border-cs-border/60 pl-2 max-h-72 overflow-y-auto">
              {projectsLoading ? (
                <div className="flex items-center gap-2 px-2 py-1.5 text-[11px] text-cs-muted">
                  <Loader2 size={10} className="animate-spin" /> Loading…
                </div>
              ) : projects.length === 0 ? (
                <button
                  onClick={() => onNavigate("settings")}
                  className="block w-full px-2 py-1.5 text-left text-[11px] text-cs-muted hover:text-cs-text"
                >
                  {t("sidebar.noProjectsYet", "No projects yet — open Settings")}
                </button>
              ) : (
                projects.map((project) => {
                  const selected = activeProject?.id === project.id;
                  return (
                    <button
                      key={project.id}
                      onClick={() => setActiveProjectStore(project)}
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
      </div>

      {/* Main nav (6 sections) */}
      <nav
        className="flex-1 py-3 px-2 space-y-0.5 overflow-y-auto"
        aria-label="Main navigation"
        role="navigation"
      >
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const isActive = active === item.id;
          // Cron alerts surface on the Runs entry now (cron lives under Runs → Schedules).
          const showAlertDot = item.id === "runs" && cronAlertCount > 0;
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
              {showAlertDot && (
                <span className="w-2 h-2 rounded-full bg-red-500 animate-pulse" />
              )}
            </button>
          );
        })}
      </nav>

      {/* Language switcher */}
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

      {/* Footer: account / login */}
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
            {t("sidebar.signInForPro", "Sign in for Pro")}
          </button>
        )}
      </div>

      {showLogin && <LoginModal onClose={() => setShowLogin(false)} />}
    </aside>
  );
}

function RuntimeDots({
  project,
}: {
  project: {
    hasClaude: boolean;
    hasCodex: boolean;
    hasHermes: boolean;
    hasOpenclaw: boolean;
    hasGemini: boolean;
  };
}) {
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
