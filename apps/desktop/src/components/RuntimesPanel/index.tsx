import { lazy, Suspense, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import AuthMethodMatrix from "./AuthMethodMatrix";
import CreditBurnCard from "./CreditBurnCard";
import RuntimeComparison from "./RuntimeComparison";
import RemoteRuntimes from "./RemoteRuntimes";
import MonitoringToggles from "./MonitoringToggles";
import PreTrustToggle from "@/components/Settings/PreTrustToggle";

// Settings → Runtimes panel.
// v1.4.0 Polish-T5 — Adds a "Compare" sub-tab that surfaces the per-runtime
// capability matrix lifted from the old AgentManager modal. The "Setup" tab
// keeps the previous AuthMethodMatrix + RuntimeSettings stack.

const RuntimeSettings = lazy(() => import("@/components/RuntimeSettings"));

interface Props {
  onOpenApiKeys?: () => void;
}

type RuntimesTab = "setup" | "monitoring" | "compare" | "remote";

const STORAGE_KEY = "ato.subtab.settings.runtimes";

function loadInitialTab(): RuntimesTab {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (
      stored === "setup" ||
      stored === "monitoring" ||
      stored === "compare" ||
      stored === "remote"
    ) {
      return stored;
    }
  } catch {
    // ignore
  }
  return "setup";
}

export default function RuntimesPanel({ onOpenApiKeys }: Props) {
  const { t } = useTranslation();
  const [active, setActive] = useState<RuntimesTab>(loadInitialTab);

  const setTab = (id: RuntimesTab) => {
    setActive(id);
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      // ignore
    }
  };

  const tabs: { id: RuntimesTab; label: string }[] = [
    { id: "setup", label: t("subnav.runtimesSetup", "Setup") },
    { id: "monitoring", label: t("subnav.runtimesMonitoring", "Monitoring") },
    { id: "compare", label: t("subnav.runtimesCompare", "Compare") },
    { id: "remote", label: t("subnav.runtimesRemote", "Remote") },
  ];

  return (
    <div className="space-y-4">
      <nav className="flex flex-wrap gap-1 border-b border-cs-border/60 pb-2" role="tablist">
        {tabs.map((tab) => {
          const isActive = tab.id === active;
          return (
            <button
              key={tab.id}
              role="tab"
              aria-selected={isActive}
              onClick={() => setTab(tab.id)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap",
                isActive
                  ? "bg-cs-accent/10 text-cs-accent"
                  : "text-cs-muted hover:bg-cs-border/50 hover:text-cs-text"
              )}
            >
              {tab.label}
            </button>
          );
        })}
      </nav>

      {active === "setup" && (
        <div className="space-y-2">
          <CreditBurnCard />
          <AuthMethodMatrix onOpenApiKeys={onOpenApiKeys} />
          <Suspense
            fallback={
              <div className="flex items-center justify-center h-32">
                <Loader2 size={20} className="animate-spin text-cs-muted" />
              </div>
            }
          >
            <RuntimeSettings />
          </Suspense>
        </div>
      )}

      {active === "monitoring" && (
        <div className="space-y-4">
          <MonitoringToggles />
          {/* F3 / S8 follow-up — claude pre-trust toggle. Self-contained
              card; lives here because dispatch-behavior settings cluster
              naturally with monitoring toggles. */}
          <PreTrustToggle />
        </div>
      )}

      {active === "compare" && <RuntimeComparison />}

      {active === "remote" && <RemoteRuntimes />}
    </div>
  );
}
