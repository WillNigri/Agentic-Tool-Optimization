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
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/hooks/useAuth";
import { useTranslation } from "react-i18next";
import { useCronStore } from "@/stores/useCronStore";
import LoginModal from "./LoginModal";

export type Section = "context" | "skills" | "subagents" | "hooks" | "automation" | "cron" | "analytics" | "mcp" | "config";

interface SidebarProps {
  active: Section;
  onNavigate: (section: Section) => void;
}

const NAV_ITEMS: { id: Section; labelKey: string; icon: typeof Layers; group?: string }[] = [
  { id: "context", labelKey: "nav.context", icon: Layers },
  { id: "skills", labelKey: "nav.skills", icon: Sparkles },
  { id: "subagents", labelKey: "nav.subagents", icon: Bot },
  { id: "hooks", labelKey: "nav.hooks", icon: Webhook },
  { id: "automation", labelKey: "nav.automation", icon: Workflow },
  { id: "cron", labelKey: "nav.cron", icon: Clock },
  { id: "analytics", labelKey: "nav.analytics", icon: BarChart3 },
  { id: "mcp", labelKey: "nav.mcp", icon: Server },
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

      <nav className="flex-1 py-3 px-2 space-y-0.5 overflow-y-auto">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              onClick={() => onNavigate(item.id)}
              className={cn(
                "w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors",
                active === item.id
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
