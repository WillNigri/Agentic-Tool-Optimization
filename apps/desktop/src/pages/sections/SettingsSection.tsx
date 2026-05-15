import { lazy, useState, Suspense } from "react";
import { useTranslation } from "react-i18next";
import {
  Settings,
  Cpu,
  Key,
  KeyRound,
  FileCode,
  Cloud as CloudIcon,
  FolderKanban,
  Archive,
  Info,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import SectionTabs, { type TabDef } from "./SectionTabs";

const RuntimesPanel = lazy(() => import("@/components/RuntimesPanel"));
const ModelConfig = lazy(() =>
  import("@/components/ModelConfig").then((m) => ({ default: m.ModelConfig }))
);
const LlmApiKeys = lazy(() => import("@/components/LlmApiKeys/LlmApiKeys"));
const SecretsManager = lazy(() =>
  import("@/components/SecretsManager").then((m) => ({ default: m.SecretsManager }))
);
const EnvManager = lazy(() =>
  import("@/components/EnvManager").then((m) => ({ default: m.EnvManager }))
);
const ProjectManager = lazy(() =>
  import("@/components/AgentManager").then((m) => ({ default: m.ProjectManager }))
);
const ConfigBackup = lazy(() => import("@/components/ConfigBackup"));
const AboutPanel = lazy(() => import("@/components/AboutPanel"));

// Cloud is a meta-tab that nests auth + teams + sync + provider keys + notifications.
const CloudAuth = lazy(() => import("@/components/CloudAuth"));
const TeamWorkspaces = lazy(() => import("@/components/TeamWorkspaces"));
const SkillSync = lazy(() => import("@/components/SkillSync"));
const ProviderKeys = lazy(() => import("@/components/ProviderKeys"));
const NotificationsSettings = lazy(() => import("@/components/NotificationsSettings"));

type CloudTabId = "auth" | "teams" | "sync" | "providerKeys" | "notifications";

function CloudTab() {
  const { t } = useTranslation();
  const [active, setActive] = useState<CloudTabId>(() => {
    try {
      const stored = localStorage.getItem("ato.subtab.settings.cloud");
      if (
        stored === "auth" ||
        stored === "teams" ||
        stored === "sync" ||
        stored === "providerKeys" ||
        stored === "notifications"
      )
        return stored;
    } catch {
      // ignore
    }
    return "auth";
  });

  const setTab = (id: CloudTabId) => {
    setActive(id);
    try {
      localStorage.setItem("ato.subtab.settings.cloud", id);
    } catch {
      // ignore
    }
  };

  const tabs: { id: CloudTabId; label: string }[] = [
    { id: "auth", label: t("subnav.cloudAuth", "Account") },
    { id: "teams", label: t("subnav.cloudTeams", "Teams") },
    { id: "sync", label: t("subnav.cloudSync", "Sync") },
    { id: "providerKeys", label: t("subnav.cloudProviderKeys", "Provider Keys") },
    { id: "notifications", label: t("subnav.cloudNotifications", "Notifications") },
  ];

  const Panel =
    active === "auth"
      ? CloudAuth
      : active === "teams"
      ? TeamWorkspaces
      : active === "sync"
      ? SkillSync
      : active === "providerKeys"
      ? ProviderKeys
      : NotificationsSettings;

  return (
    <div className="space-y-4">
      <nav className="flex flex-wrap gap-1 border-b border-cs-border/60 pb-2" role="tablist">
        {tabs.map((t) => {
          const isActive = t.id === active;
          return (
            <button
              key={t.id}
              role="tab"
              aria-selected={isActive}
              onClick={() => setTab(t.id)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap",
                isActive
                  ? "bg-cs-accent/10 text-cs-accent"
                  : "text-cs-muted hover:bg-cs-border/50 hover:text-cs-text"
              )}
            >
              {t.label}
            </button>
          );
        })}
      </nav>
      <Suspense
        fallback={
          <div className="flex items-center justify-center h-32">
            <Loader2 size={20} className="animate-spin text-cs-muted" />
          </div>
        }
      >
        <Panel />
      </Suspense>
    </div>
  );
}

export default function SettingsSection() {
  const { t } = useTranslation();
  const tabs: TabDef[] = [
    {
      id: "runtimes",
      label: t("subnav.settingsRuntimes", "Runtimes"),
      icon: Settings,
      Component: RuntimesPanel,
    },
    {
      id: "models",
      label: t("subnav.settingsModels", "Models"),
      icon: Cpu,
      Component: ModelConfig,
    },
    {
      id: "api-keys",
      label: t("subnav.settingsApiKeys", "API Keys"),
      icon: Key,
      Component: LlmApiKeys,
    },
    {
      id: "secrets",
      label: t("subnav.settingsSecrets", "Secrets"),
      icon: KeyRound,
      Component: SecretsManager,
    },
    {
      id: "env",
      label: t("subnav.settingsEnv", "Environment"),
      icon: FileCode,
      Component: EnvManager,
    },
    {
      id: "cloud",
      label: t("subnav.settingsCloud", "Cloud"),
      icon: CloudIcon,
      Component: CloudTab,
    },
    {
      id: "projects",
      label: t("subnav.settingsProjects", "Projects"),
      icon: FolderKanban,
      Component: ProjectManager,
    },
    {
      id: "backup",
      label: t("subnav.settingsBackup", "Backup"),
      icon: Archive,
      Component: ConfigBackup,
    },
    {
      id: "about",
      label: t("subnav.settingsAbout", "About"),
      icon: Info,
      Component: AboutPanel,
    },
  ];
  return <SectionTabs storageKey="ato.subtab.settings" tabs={tabs} />;
}
